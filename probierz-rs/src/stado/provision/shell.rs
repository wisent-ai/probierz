use serde_json::json;
use crate::stado::*;
pub(crate) fn sh(
    command: &str,
    args: &[String],
    cwd: Option<&Path>,
    host: Option<&discovery::Host>,
    timeout: Option<Duration>,
) -> ProcessOutput {
    let mut process = Command::new(command);
    process
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(directory) = cwd {
        process.current_dir(directory);
    }
    if let Some(api_url) = host.and_then(|entry| entry.api_url) {
        process.env("STADO_API_URL", api_url);
    }
    let display_args = args.to_vec();
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ProcessOutput {
                command: command.to_string(),
                args: display_args,
                status: None,
                signal: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(error.to_string()),
            }
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stdout {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stderr {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    });
    let started = Instant::now();
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (Some(status), false),
            Ok(None)
                if timeout
                    .map(|limit| started.elapsed() >= limit)
                    .unwrap_or(false) =>
            {
                let _ = child.kill();
                break (child.wait().ok(), true);
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                let _ = child.kill();
                return ProcessOutput {
                    command: command.to_string(),
                    args: display_args,
                    status: None,
                    signal: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error: Some(error.to_string()),
                };
            }
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    ProcessOutput {
        command: command.to_string(),
        args: display_args,
        status: status.as_ref().and_then(|value| value.code()),
        signal: status.as_ref().and_then(|value| value.signal()),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        error: timed_out.then(|| "operation timed out".to_string()),
    }
}

pub(crate) fn process_text(output: &ProcessOutput) -> String {
    [
        output.error.as_deref(),
        Some(output.stderr.as_str()),
        Some(output.stdout.as_str()),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
    .trim()
    .to_string()
}

pub(crate) fn process_record(output: &ProcessOutput) -> Value {
    json!({
        "status": output.status,
        "signal": output.signal,
        "error": output.error.as_ref().map(|message| json!({ "code": Value::Null, "message": message })),
        "stdout": output.stdout,
        "stderr": output.stderr,
    })
}

pub(crate) fn remote_failure(point: &str, action: &str, output: &ProcessOutput) -> Failure {
    let diagnostic = json!({
        "failure_point": point,
        "command": output.command,
        "args": output.args,
        "exit_code": output.status,
        "stdout": output.stdout,
        "stderr": output.stderr,
        "error": output.error,
    });
    eprintln!("probierz-process-failure {diagnostic}");
    let exit = output
        .status
        .map(|code| code.to_string())
        .unwrap_or_else(|| "none".to_string());
    let text = process_text(output);
    Failure::unavailable(
        point,
        format!(
            "{action}: {STADO_BIN} exit {exit}{}",
            if text.is_empty() {
                String::new()
            } else {
                format!(" — {text}")
            }
        ),
    )
}

pub(crate) fn local_failure(point: &str, action: &str, output: &ProcessOutput) -> Failure {
    let exit = output
        .status
        .map(|code| code.to_string())
        .unwrap_or_else(|| "none".to_string());
    let text = process_text(output);
    Failure::config(
        point,
        format!(
            "{action}: tar exit {exit}{}",
            if text.is_empty() {
                String::new()
            } else {
                format!(" — {text}")
            }
        ),
    )
}

pub(crate) fn failure_summary(failure: &Failure, message: impl Into<String>) -> Value {
    json!({
        "failurePoint": failure.point,
        "errorCode": failure.code.as_str(),
        "service": "probierz",
        "retryable": failure.code.retryable(),
        "outage": failure.code.retryable(),
        "message": message.into(),
    })
}

pub(crate) fn host(name: &str, point: &str) -> Result<discovery::Host, Failure> {
    discovery::stado_host(name).ok_or_else(|| {
        Failure::config(
            point,
            format!("No such stado host: \"{name}\". Run `probierz hosts` for the list."),
        )
    })
}

pub(crate) fn require_gui_ready(target: &str, selected: &discovery::Host) -> Answer {
    if target != "desktop:cua" {
        return Ok(());
    }
    let registry_target = selected.target.ok_or_else(|| {
        Failure::config(
            "stado.preflight",
            format!(
                "The selected host \"{}\" cannot prove a usable macOS GUI session.",
                selected.host
            ),
        )
    })?;
    let started = Instant::now();
    let output = sh(
        STADO_BIN,
        &[
            "host".into(),
            "gui-automation".into(),
            "status".into(),
            registry_target.into(),
        ],
        None,
        Some(selected),
        Some(GUI_STATUS_TIMEOUT),
    );
    if output.error.as_deref() == Some("operation timed out") {
        return Err(Failure::config(
            "stado.preflight",
            format!("The GUI readiness audit for {registry_target} exceeded its deadline. Readiness is unknown; no GUI job was submitted. elapsed_ms={}", started.elapsed().as_millis()),
        ));
    }
    if output.status != Some(0) {
        return Err(remote_failure(
            "stado.preflight",
            &format!("Reading GUI readiness for {registry_target} failed"),
            &output,
        ));
    }
    let mut fields = BTreeMap::new();
    for line in output.stdout.lines() {
        let parts: Vec<&str> = line.trim().split('\t').collect();
        if parts.len() >= 3 {
            fields.insert(parts[1], parts[2..].join("\t"));
        }
    }
    let console = fields
        .get("console")
        .map(String::as_str)
        .unwrap_or("unknown");
    let accessibility = fields
        .get("accessibility")
        .map(String::as_str)
        .unwrap_or("unknown");
    let ready = !matches!(console, "" | "root" | "loginwindow" | "unknown")
        && fields.get("accessibility-user").map(String::as_str) == Some(console)
        && fields.get("automated-session-declared").map(String::as_str) == Some("yes")
        && fields.get("cua-driver-app").map(String::as_str) == Some("present")
        && accessibility == "granted";
    if !ready {
        return Err(Failure::config(
            "stado.preflight",
            format!("The selected host \"{}\" is not ready for desktop:cua: it needs an active macOS console session and a granted CuaDriver.", selected.host),
        ));
    }
    Ok(())
}

pub(crate) fn state_uri(kind: &str) -> String {
    format!("stado://probierz/{kind}")
}

