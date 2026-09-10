use serde_json::json;
use crate::run::*;
use crate::run::report::identity::Mode600;
pub(crate) fn run_data_command(
    harness: &Path,
    config: Option<&serde_yaml::Value>,
    env: &BTreeMap<String, String>,
    secrets: &[(String, String)],
    stdout: &Path,
    stderr: &Path,
) -> Value {
    let Some(config) = config else {
        return json!({ "ok": true, "result": null });
    };
    let args = match config.get("args") {
        Some(value) => match value.as_sequence() {
            Some(values) => values
                .iter()
                .map(|value| yaml_string(value).unwrap_or_default())
                .collect::<Vec<_>>(),
            None => return json!({ "ok": false, "error": "invalid data command configuration" }),
        },
        None => Vec::new(),
    };
    if let Some(capability) = config.get("capability").and_then(serde_yaml::Value::as_str) {
        return match crate::apphooks::execute(harness, capability, &args, env) {
            Ok(result) => json!({ "ok": true, "result": result }),
            Err(error) => json!({ "ok": false, "error": error.detail }),
        };
    }
    let Some(command) = config.get("command").and_then(serde_yaml::Value::as_str) else {
        return json!({ "ok": false, "error": "invalid data command configuration" });
    };
    let cwd = config
        .get("cwd")
        .and_then(serde_yaml::Value::as_str)
        .map(|cwd| normalize_path(&harness.join(cwd)))
        .unwrap_or_else(|| harness.to_path_buf());
    let timeout = config
        .get("timeoutMs")
        .and_then(serde_yaml::Value::as_u64)
        .filter(|value| *value > 0)
        .unwrap_or(120_000);
    let execution = capture(command, &args, Some(&cwd), Some(env), Some(timeout));
    let safe_out = redact_text(&text(&execution.stdout), secrets);
    let safe_err = redact_text(&text(&execution.stderr), secrets);
    if !safe_out.is_empty() {
        let _ = append_secure(stdout, stamped(&safe_out).as_bytes());
    }
    if !safe_err.is_empty() {
        let _ = append_secure(stderr, stamped(&safe_err).as_bytes());
    }
    if execution.error.is_some() || !execution.status.is_some_and(|status| status.success()) {
        let detail = execution.error.unwrap_or_else(|| {
            let trimmed = safe_err.trim();
            if trimmed.is_empty() {
                format!(
                    "exit {}",
                    execution
                        .status
                        .and_then(|status| status.code())
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "null".into())
                )
            } else {
                trimmed.into()
            }
        });
        return json!({ "ok": false, "error": tail_chars(&detail, TAIL), "exitCode": execution.status.and_then(|status| status.code()) });
    }
    let stdout_text = text(&execution.stdout);
    let lines: Vec<&str> = stdout_text
        .lines()
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return json!({ "ok": true, "result": null });
    }
    match serde_json::from_str::<Value>(lines.last().expect("not empty")) {
        Ok(result) => json!({ "ok": true, "result": result }),
        Err(_) => json!({ "ok": false, "error": "data command did not return JSON" }),
    }
}
pub(crate) fn append_secure(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).append(true).mode_600();
    options.open(path)?.write_all(bytes)
}

