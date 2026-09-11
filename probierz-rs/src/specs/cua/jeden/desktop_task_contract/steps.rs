use super::*;
use serde_json::json;
pub(crate) fn record_backend(
    context: &specs::Context,
    workspace_name: &str,
    workspace_root: &Path,
    sessions_root: &Path,
    task: &str,
    trace: &mut Vec<Value>,
) -> Result<Backend, String> {
    fs::create_dir_all(sessions_root)
        .map_err(|error| format!("{}: {error}", sessions_root.display()))?;
    if workspace_root.exists() {
        return Err(format!(
            "The job-owned workspace already exists and cannot be treated as isolated: {}",
            workspace_root.display()
        ));
    }
    fs::create_dir(workspace_root)
        .map_err(|error| format!("{}: {error}", workspace_root.display()))?;
    let home = std::env::var("HOME")
        .map_err(|_| "HOME is required for the Jeden Desktop task-contract journey".to_string())?;
    let command = PathBuf::from(home).join(".stado/bin/stado");
    let args = [
        "host",
        "jeden-connect",
        workspace_name,
        "--target",
        DEDICATED_HOST,
    ];
    let stderr = Arc::new(Mutex::new(String::new()));
    let mut launch = Command::new(&command);
    launch
        .args(args)
        .current_dir(workspace_root)
        .env("JEDEN_LANGUAGE", "en")
        .env("JEDEN_SESSION_ROOT", sessions_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in [
        "STADO_MODEL_ROUTER_TOKEN",
        "PROBIERZ_MODEL_AGENT_SECRET",
        "PROBIERZ_SOURCE_IDENTITY",
        "PROBIERZ_APP_SOURCE",
        "WC_JOB_ID",
    ] {
        if let Some(value) = context.optional(name) {
            launch.env(name, value);
        }
    }
    let mut child = launch
        .spawn()
        .map_err(|error| format!("{}: {error}", command.display()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "The real Stado/Jeden connection has no stdout".to_string())?;
    let child_stderr = child
        .stderr
        .take()
        .ok_or_else(|| "The real Stado/Jeden connection has no stderr".to_string())?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "The real Stado/Jeden connection has no stdin".to_string())?;
    let (sender, receiver) = mpsc::channel::<Result<String, String>>();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender
                .send(line.map_err(|error| error.to_string()))
                .is_err()
            {
                break;
            }
        }
    });
    let stderr_sink = Arc::clone(&stderr);
    thread::spawn(move || {
        let mut reader = BufReader::new(child_stderr);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            stderr_sink.lock().expect("stderr lock").push_str(&line);
            line.clear();
        }
    });

    let deadline = Instant::now() + REPORT_TIMEOUT;
    let mut contract = None;
    let mut session_path = None;
    let mut rpc_session = None;
    let mut prompt_completed = false;
    let mut shutdown_acknowledged = false;
    let mut requests = Vec::new();
    let mut frames = Vec::new();
    let outcome = (|| {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("The real Stado/Jeden task did not finish within 150000 ms".to_string());
            }
            match receiver.recv_timeout(remaining.min(Duration::from_millis(500))) {
                Ok(Ok(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let frame: Value = serde_json::from_str(line.trim()).map_err(|error| {
                        format!("Jeden returned a non-JSON frame: {error}: {line}")
                    })?;
                    frames.push(frame.clone());
                    let method = frame.get("method").and_then(Value::as_str);
                    if matches!(
                        method,
                        Some("session/request_permission" | "session/request_input")
                    ) {
                        return Err(format!(
                            "The read-only report task unexpectedly requested interaction: {frame}"
                        ));
                    }
                    if frame.get("type").and_then(Value::as_str) == Some("ready")
                        && contract.is_none()
                    {
                        write_frame(
                            &mut stdin,
                            &json!({"id":"contract-oracle","method":"config/contracts/get","params":{}}),
                            &mut requests,
                        )?;
                    } else if frame.get("id").and_then(Value::as_str) == Some("contract-oracle") {
                        if !frame.get("error").unwrap_or(&Value::Null).is_null() {
                            return Err(format!("Jeden did not return the task contract: {frame}"));
                        }
                        let received = frame
                            .pointer("/result/taskContract")
                            .cloned()
                            .unwrap_or(Value::Null);
                        if received.get("version").and_then(Value::as_u64) != Some(1) {
                            return Err(
                                "The real backend must expose task contract version 1".to_string()
                            );
                        }
                        if sorted_requirement_ids(&received) != required_ids() {
                            return Err("The real backend task contract must expose the seven required report entries".to_string());
                        }
                        contract = Some(received);
                        write_frame(
                            &mut stdin,
                            &json!({"id":"session-open","method":"session/new","params":{"options":{"allowWrite":false,"allowCommand":false,"autoApprove":false,"maxSteps":24}}}),
                            &mut requests,
                        )?;
                    } else if frame.get("id").and_then(Value::as_str) == Some("session-open") {
                        if !frame.get("error").unwrap_or(&Value::Null).is_null() {
                            return Err(format!(
                                "Jeden could not create the isolated session: {frame}"
                            ));
                        }
                        rpc_session = frame
                            .pointer("/result/sessionId")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        session_path = frame
                            .pointer("/result/sessionPath")
                            .and_then(Value::as_str)
                            .map(PathBuf::from);
                        if rpc_session.is_none() || session_path.is_none() {
                            return Err(
                                "Jeden returned an incomplete session/new response".to_string()
                            );
                        }
                        write_frame(
                            &mut stdin,
                            &json!({"id":"report-turn","method":"session/prompt","params":{"sessionId":rpc_session,"requestId":"task-contract-native-report","prompt":task}}),
                            &mut requests,
                        )?;
                    } else if frame.get("id").and_then(Value::as_str) == Some("report-turn") {
                        if !frame.get("error").unwrap_or(&Value::Null).is_null() {
                            return Err(format!(
                                "The real task failed: {}",
                                frame.get("error").unwrap()
                            ));
                        }
                        prompt_completed = true;
                        write_frame(
                            &mut stdin,
                            &json!({"id":"shutdown","method":"shutdown","params":{}}),
                            &mut requests,
                        )?;
                    } else if frame.get("id").and_then(Value::as_str) == Some("shutdown") {
                        if !frame.get("error").unwrap_or(&Value::Null).is_null() {
                            return Err(format!("Jeden refused shutdown: {frame}"));
                        }
                        shutdown_acknowledged = true;
                        break;
                    }
                }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if child
                        .try_wait()
                        .map_err(|error| error.to_string())?
                        .is_some()
                    {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        drop(stdin);
        let remaining = deadline.saturating_duration_since(Instant::now());
        let status = wait_child(&mut child, remaining)?;
        if !status.success() {
            return Err(format!(
                "The real Stado/Jeden connection exited unsuccessfully: {}",
                stderr.lock().expect("stderr lock")
            ));
        }
        if !prompt_completed {
            return Err("The real session/prompt response was not received".to_string());
        }
        if !shutdown_acknowledged {
            return Err("The real Jeden RPC did not acknowledge shutdown".to_string());
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        let _ = child.kill();
        trace.push(json!({"label":"real-backend-task","command":command,"args":args,"workspaceRoot":workspace_root,"sessionsRoot":sessions_root,"requests":requests,"frames":frames,"stderr":*stderr.lock().expect("stderr lock"),"error":error}));
        return Err(error);
    }
    trace.push(json!({"label":"real-backend-task","command":command,"args":args,"workspaceRoot":workspace_root,"sessionsRoot":sessions_root,"requests":requests,"frames":frames,"stderr":*stderr.lock().expect("stderr lock"),"status":0}));
    Ok(Backend {
        contract: contract.unwrap_or(Value::Null),
        session_path: session_path.unwrap_or_default(),
    })
}
