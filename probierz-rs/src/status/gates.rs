//! The release gate of an application and the evidence level a run reaches.

use super::*;

pub(super) fn gate_status(loaded: &manifest::Manifest, app_id: &str) -> Result<Value, Failure> {
    let file = loaded
        .file
        .parent()
        .unwrap_or(Path::new("."))
        .join("gates.json");
    let exists = file.exists();
    let mut config = if exists {
        serde_json::from_str::<Value>(&fs::read_to_string(&file)?)?
    } else {
        json!({
            "schemaVersion": 2,
            "appId": app_id,
            "modes": {
                "pull-request": { "enforcement": "pending-green" },
                "release": { "enforcement": "pending-green" },
            },
        })
    };
    let object = config.as_object_mut().ok_or_else(|| {
        Failure::config(
            "status.gate",
            format!("gate config is not an object: {}", file.display()),
        )
    })?;
    object.insert(
        "file".to_string(),
        Value::String(file.to_string_lossy().into_owned()),
    );
    object.insert("exists".to_string(), Value::Bool(exists));
    Ok(config)
}

pub(super) fn evidence_level(run: &Value) -> &'static str {
    if run.get("status").and_then(Value::as_str) != Some("passed") {
        return "E0";
    }
    let recorded = run
        .pointer("/conditions/record")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let report = run
        .pointer("/evidence/report")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let analysis = run
        .pointer("/evidence/analysis")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let capture = run
        .pointer("/evidence/capturePresent")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if recorded && report && analysis && capture {
        "E3"
    } else {
        "E2"
    }
}

pub(super) fn evidence_rank(level: &str) -> i32 {
    match level {
        "E0" => 0,
        "E1" => 1,
        "E2" => 2,
        "E3" => 3,
        _ => -1,
    }
}
