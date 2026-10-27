use crate::run::report::identity::Mode600;
use crate::run::*;
use serde_json::json;
pub(crate) fn run_data_command(
    harness: &Path,
    config: Option<&serde_yaml::Value>,
    env: &BTreeMap<String, String>,
    redaction: &manifest::Redaction,
    stdout: &Path,
    stderr: &Path,
) -> Value {
    let Some(config) = config else {
        return json!({ "ok": true, "result": null });
    };
    let args = match config.get("args") {
        Some(value) => match value.as_sequence() {
            Some(values) => match values.iter().map(yaml_string).collect::<Option<Vec<_>>>() {
                Some(args) => args,
                // Passed as '' it ran the command with an argument nobody wrote.
                None => {
                    return json!({ "ok": false, "error": "a data command argument is not a scalar value" })
                }
            },
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
    // A data command runs in the harness unless it names its own directory,
    // relative to the harness; a `cwd` that is not a path is refused rather
    // than run in the harness as if none had been named.
    let cwd = match config.get("cwd") {
        None => harness.to_path_buf(),
        Some(value) => match value.as_str() {
            Some(cwd) => normalize_path(&harness.join(cwd)),
            None => {
                return json!({ "ok": false, "error": format!("data command cwd must be a path, not {value:?}") })
            }
        },
    };
    let execution = capture(command, &args, Some(&cwd), Some(env));
    let safe_out = redaction.text(&text(&execution.stdout));
    let safe_err = redaction.text(&text(&execution.stderr));
    let mut failures = Vec::new();
    if !safe_out.is_empty() {
        if let Err(error) = append_secure(stdout, stamped(&safe_out).as_bytes()) {
            failures.push(format!(
                "retain data-command stdout at {}: {error}",
                stdout.display()
            ));
        }
    }
    if !safe_err.is_empty() {
        if let Err(error) = append_secure(stderr, stamped(&safe_err).as_bytes()) {
            failures.push(format!(
                "retain data-command stderr at {}: {error}",
                stderr.display()
            ));
        }
    }
    if let Some(error) = execution.error {
        failures.push(format!(
            "capture data command {command}: {}",
            redaction.text(&error)
        ));
    }
    match execution.status {
        Some(status) if status.success() => {}
        Some(status) => failures.push(format!(
            "data command {command} {}; stderr: {safe_err}",
            crate::failure::ended(&status)
        )),
        None => failures.push(format!(
            "data command {command} has no observed exit status"
        )),
    }
    if !failures.is_empty() {
        return json!({
            "ok": false,
            "error": failures.join("; "),
            "exitCode": execution.status.and_then(|status| status.code()),
        });
    }
    let stdout_text = text(&execution.stdout);
    // A command that printed nothing answers no result; otherwise its last
    // non-empty line is the JSON it answers with.
    let Some(last) = stdout_text.lines().filter(|line| !line.is_empty()).last() else {
        return json!({ "ok": true, "result": null });
    };
    match serde_json::from_str::<Value>(last) {
        Ok(result) => json!({ "ok": true, "result": result }),
        Err(error) => json!({
            "ok": false,
            "error": format!("decode data command {command} result as JSON: {error}"),
        }),
    }
}
pub(crate) fn append_secure(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).append(true).mode_600();
    options.open(path)?.write_all(bytes)
}
