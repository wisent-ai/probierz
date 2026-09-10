use serde_json::json;
use crate::run::*;
pub(crate) fn report_identity(path: &Path, run_id: &str, started: SystemTime) -> Value {
    if !path.exists() {
        return json!({ "ok": false, "error": "report missing" });
    }
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) => return json!({ "ok": false, "error": format!("report unreadable: {error}") }),
    };
    if meta
        .modified()
        .ok()
        .is_some_and(|modified| modified < started)
    {
        return json!({ "ok": false, "error": "report predates run start" });
    }
    match fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    {
        Some(report) => {
            let actual = report.pointer("/probierz/runId").and_then(Value::as_str);
            if actual != Some(run_id) {
                json!({ "ok": false, "error": format!("report run ID mismatch: expected {run_id}, got {}", actual.unwrap_or("missing")) })
            } else {
                json!({ "ok": true, "runId": run_id, "mtime": meta.modified().map(system_time_iso).unwrap_or_else(|_| now_iso()) })
            }
        }
        None => json!({ "ok": false, "error": "report unreadable: invalid JSON" }),
    }
}

pub(crate) fn redact_diagnostic(value: &str) -> String {
    Regex::new(r"([?&][^=\s&]+)=([^&\s]+)")
        .expect("regex")
        .replace_all(&safe_message(value), "$1=[VALUE]")
        .into_owned()
}

pub(crate) fn simulator_identifier(requested: Option<&str>) -> String {
    let requested = requested.unwrap_or("booted");
    if requested == "booted" {
        return requested.into();
    }
    let document = simctl_devices();
    let candidates: Vec<&Value> = document
        .get("devices")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|devices| devices.values())
        .filter_map(Value::as_array)
        .flatten()
        .filter(|device| {
            device.get("udid").and_then(Value::as_str) == Some(requested)
                || device.get("name").and_then(Value::as_str) == Some(requested)
        })
        .collect();
    candidates
        .iter()
        .find(|device| device.get("state").and_then(Value::as_str) == Some("Booted"))
        .copied()
        .or_else(|| candidates.first().copied())
        .and_then(|device| device.get("udid").and_then(Value::as_str))
        .unwrap_or(requested)
        .to_string()
}

pub(crate) fn write_secure_text(path: &Path, value: &str) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true).mode_600();
    options.open(path)?.write_all(value.as_bytes())
}

