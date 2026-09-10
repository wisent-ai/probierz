use crate::run::*;
pub(crate) fn start_byk_broker(
    harness: &Path,
    env: &BTreeMap<String, String>,
    secrets: &[(String, String)],
    stdout_path: &Path,
    stderr_path: &Path,
    timeout_ms: u64,
) -> Result<BykBroker, String> {
    // The broker is provisioned, not built: see `byk_broker_binary`.
    let (broker, rotator, broker_env) = byk_broker_binary(harness, env, timeout_ms)?;
    let _ = (secrets, stdout_path, stderr_path);

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("probierz-byk-auth-{}-{stamp}", std::process::id()));
    fs::create_dir(&directory).map_err(|error| error.to_string())?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let socket_path = directory.join("byk-otp.sock");
    let mut command = Command::new(&broker);
    command
        .args([
            "mailbox-broker",
            "--mailbox",
            "byk-ios-login",
            "--socket",
            socket_path.to_string_lossy().as_ref(),
        ])
        .current_dir(&rotator)
        .envs(&broker_env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            let _ = fs::remove_dir_all(&directory);
            return Err("could not start the Skarbiec mailbox broker".into());
        }
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "could not read the Skarbiec mailbox broker".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "could not read the Skarbiec mailbox broker".to_string())?;
    let stderr_tail = Arc::new(Mutex::new(Vec::new()));
    let stderr_capture = Arc::clone(&stderr_tail);
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut buffer = [0_u8; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            if let Ok(mut tail) = stderr_capture.lock() {
                tail.extend_from_slice(&buffer[..count]);
                if tail.len() > TAIL {
                    let remove = tail.len() - TAIL;
                    tail.drain(..remove);
                }
            }
        }
    });
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line).and_then(|count| {
            if count == 0 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "broker stdout closed",
                ))
            } else {
                Ok(line)
            }
        });
        let _ = sender.send(result);
        let _ = std::io::copy(&mut reader, &mut std::io::sink());
    });
    let mut owner = BykBroker {
        child,
        directory,
        socket_path,
        recipient: String::new(),
    };
    let line = match receiver.recv_timeout(Duration::from_millis(15_000)) {
        Ok(Ok(line)) if line.len() <= 16_384 => line.trim_end_matches(['\r', '\n']).to_string(),
        Ok(Ok(_)) => {
            return Err(byk_startup_error(
                "Skarbiec mailbox broker readiness line was too large",
                &stderr_tail,
            ));
        }
        Ok(Err(_)) => {
            return Err(byk_startup_error(
                "Skarbiec mailbox broker exited before readiness",
                &stderr_tail,
            ));
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(byk_startup_error(
                "timed out waiting for the Skarbiec mailbox broker",
                &stderr_tail,
            ));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(byk_startup_error(
                "Skarbiec mailbox broker exited before readiness",
                &stderr_tail,
            ));
        }
    };
    let readiness: Value = serde_json::from_str(&line).map_err(|_| {
        byk_startup_error(
            "Skarbiec mailbox broker returned invalid readiness JSON",
            &stderr_tail,
        )
    })?;
    if !readiness.is_object()
        || readiness.get("status").and_then(Value::as_str) != Some("ready")
        || readiness.get("mailbox").and_then(Value::as_str) != Some("byk-ios-login")
    {
        return Err(byk_startup_error(
            "Skarbiec mailbox broker returned invalid readiness data",
            &stderr_tail,
        ));
    }
    let socket = readiness
        .get("socket_path")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            byk_startup_error(
                "Skarbiec mailbox broker returned an invalid socket path",
                &stderr_tail,
            )
        })?;
    if !Path::new(socket).is_absolute() || Path::new(socket) != owner.socket_path {
        return Err(byk_startup_error(
            "Skarbiec mailbox broker returned an invalid socket path",
            &stderr_tail,
        ));
    }
    let recipient = readiness
        .get("recipient")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            byk_startup_error(
                "Skarbiec mailbox broker returned an invalid recipient",
                &stderr_tail,
            )
        })?;
    if !valid_byk_recipient(recipient) {
        return Err(byk_startup_error(
            "Skarbiec mailbox broker returned an invalid recipient",
            &stderr_tail,
        ));
    }
    if !fs::metadata(&owner.socket_path)
        .map(|value| value.file_type().is_socket())
        .unwrap_or(false)
    {
        return Err(byk_startup_error(
            "Skarbiec mailbox broker did not create a Unix socket",
            &stderr_tail,
        ));
    }
    owner.recipient = recipient.to_string();
    Ok(owner)
}
