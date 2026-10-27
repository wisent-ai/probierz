use crate::run::*;
use serde_json::json;
/// Whether the report at `path` is this run's: written after the run started
/// and naming its run ID. Each refusal states what was observed; a report whose
/// modification time cannot be read is refused rather than taken as fresh, and
/// a read failure keeps its cause instead of being called invalid JSON.
pub(crate) fn report_identity(path: &Path, run_id: &str, started: SystemTime) -> Value {
    let refused = |error: String| json!({ "ok": false, "error": error });
    let modified = match fs::metadata(path).and_then(|meta| meta.modified()) {
        Ok(modified) => modified,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return refused("report missing".into())
        }
        Err(error) => return refused(format!("report unreadable: {error}")),
    };
    if modified < started {
        return refused("report predates run start".into());
    }
    let report = match crate::json_file::read(path) {
        Ok(Some(report)) => report,
        Ok(None) => return refused("report missing".into()),
        Err(error) => return refused(format!("report unreadable: {error}")),
    };
    match report.pointer("/probierz/runId").and_then(Value::as_str) {
        Some(actual) if actual == run_id => {
            json!({ "ok": true, "runId": run_id, "mtime": system_time_iso(modified) })
        }
        Some(actual) => refused(format!(
            "report run ID mismatch: expected {run_id}, got {actual}"
        )),
        None => refused(format!(
            "report run ID mismatch: expected {run_id}, the report states none"
        )),
    }
}

pub(crate) fn redact_diagnostic(value: &str, redaction: &manifest::Redaction) -> String {
    Regex::new(r"([?&][^=\s&]+)=([^&\s]+)")
        .expect("regex")
        .replace_all(&safe_message(value, redaction), "$1=[VALUE]")
        .into_owned()
}

/// simctl's own name for whichever simulator is booted.
const BOOTED: &str = "booted";

pub(crate) fn simulator_identifier(requested: Option<&str>) -> String {
    // A run that names no simulator, or names the booted one, uses simctl's alias.
    let Some(requested) = requested.filter(|requested| *requested != BOOTED) else {
        return BOOTED.into();
    };
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
    let chosen = candidates
        .iter()
        .find(|device| device.get("state").and_then(Value::as_str) == Some("Booted"))
        .or(candidates.first())
        .copied();
    match chosen.and_then(|device| device.get("udid").and_then(Value::as_str)) {
        Some(udid) => udid.to_string(),
        // A simulator simctl does not list goes to simctl as the run named it,
        // and simctl refuses it by that name.
        None => requested.to_string(),
    }
}

pub(crate) fn write_secure_text(path: &Path, value: &str) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true).mode_600();
    options.open(path)?.write_all(value.as_bytes())
}
