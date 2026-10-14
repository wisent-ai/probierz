use crate::stado::*;
use serde_json::json;
/// Run `command` to its end and keep its output. The child is waited for,
/// never polled or killed on a timer: a command that fails reports its own
/// exit and stderr.
pub(crate) fn sh(command: &str, args: &[String], cwd: Option<&Path>) -> ProcessOutput {
    let mut process = Command::new(command);
    process
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(directory) = cwd {
        process.current_dir(directory);
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
    let waited = stado_wait::begin(
        stado_wait::Kind::Process,
        format!("{command} {}", display_args.join(" ")),
        "this host",
    )
    .settle(child.wait());
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    let (status, error) = match waited {
        Ok(status) => (Some(status), None),
        Err(error) => (None, Some(format!("waiting for {command} failed: {error}"))),
    };
    ProcessOutput {
        command: command.to_string(),
        args: display_args,
        status: status.as_ref().and_then(|value| value.code()),
        signal: status.as_ref().and_then(|value| value.signal()),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        error,
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
    let registry_target = selected.target.as_deref().ok_or_else(|| {
        Failure::config(
            "stado.preflight",
            format!(
                "The selected host \"{}\" cannot prove a usable macOS GUI session.",
                selected.host
            ),
        )
    })?;
    let output = sh(
        STADO_BIN,
        &[
            "host".into(),
            "gui-automation".into(),
            "status".into(),
            registry_target.into(),
        ],
        None,
    );
    if output.status != Some(0) {
        return Err(remote_failure(
            "stado.preflight",
            &format!("Reading GUI readiness for {registry_target} failed"),
            &output,
        ));
    }
    // Stado decides whether a host is GUI-ready (`gui-ready yes|no`, from its
    // console, declared session, accessibility grant, driver and socket);
    // this preflight takes that answer instead of deciding again from the
    // rows with its own list of console owners. A status without the row is
    // refused quoting what Stado printed.
    let mut fields = BTreeMap::new();
    for line in output.stdout.lines() {
        let parts: Vec<&str> = line.trim().split('\t').collect();
        if let [_, name, value @ ..] = parts.as_slice() {
            if !value.is_empty() {
                fields.insert(*name, value.join("\t"));
            }
        }
    }
    let rows = || {
        fields
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match fields.get("gui-ready").map(String::as_str) {
        Some("yes") => Ok(()),
        Some(_) => Err(Failure::config(
            "stado.preflight",
            format!(
                "The selected host \"{}\" is not ready for desktop:cua; Stado reports {}",
                selected.host,
                rows()
            ),
        )),
        None => Err(Failure::unavailable(
            "stado.preflight",
            format!(
                "stado host gui-automation status {registry_target} printed no gui-ready row; it printed {}",
                rows()
            ),
        )),
    }
}

pub(crate) fn state_uri(kind: &str) -> String {
    format!("stado://probierz/{kind}")
}
