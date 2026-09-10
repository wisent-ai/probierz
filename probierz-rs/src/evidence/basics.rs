use serde_json::json;
use crate::evidence::*;

pub(crate) fn sha256_bytes(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

pub(crate) fn sha256_file(file: &Path) -> Result<String, Failure> {
    let mut input = File::open(file)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

pub(crate) fn canonical(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            let body = items.iter().map(canonical).collect::<Vec<_>>().join(",");
            format!("[{body}]")
        }
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            let body = keys
                .into_iter()
                .map(|key| {
                    let encoded = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string());
                    format!("{encoded}:{}", canonical(&object[key]))
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
        _ => serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()),
    }
}

pub(crate) fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

pub(crate) fn yaml_json(value: &serde_yaml::Value) -> Result<Value, Failure> {
    Ok(serde_json::to_value(value)
        .map_err(|error| Failure::config("evidence.manifest", error.to_string()))?)
}

pub(crate) fn json_file(path: &Path) -> Result<Value, Failure> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub(crate) fn try_json_file(path: &Path) -> Option<Value> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

pub(crate) fn manifests_below(root: &Path) -> Result<Vec<PathBuf>, Failure> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut pending = vec![root.to_path_buf()];
    let mut found = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() && entry.file_name() == "run-manifest.json" {
                found.push(entry.path());
            }
        }
    }
    Ok(found)
}

pub(crate) fn normalized_status(manifest: &Value) -> String {
    match manifest.get("status").and_then(Value::as_str) {
        Some("passed" | "executed") => "passed",
        Some("blocked") => "blocked",
        Some("canceled") => "canceled",
        Some("failed") => "failed",
        _ if manifest
            .get("completedAt")
            .is_some_and(|value| !value.is_null()) =>
        {
            "failed"
        }
        _ => "incomplete",
    }
    .to_string()
}

pub(crate) fn tests_from(run_directory: &Path, manifest_value: &Value) -> Vec<Value> {
    let analysis_path = manifest_value
        .get("analysisPath")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| run_directory.join("analysis.json"));
    let report_path = manifest_value
        .pointer("/paths/reportPath")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| run_directory.join("report.json"));
    let analysis = try_json_file(&analysis_path);
    let report = try_json_file(&report_path);
    let source = analysis
        .as_ref()
        .and_then(|value| value.get("tests"))
        .and_then(Value::as_array)
        .or_else(|| {
            report
                .as_ref()
                .and_then(|value| value.get("tests"))
                .and_then(Value::as_array)
        });
    let mut order = Vec::<String>::new();
    let mut by_title = HashMap::<String, Value>::new();
    for test in source.into_iter().flatten() {
        let title = test
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !by_title.contains_key(&title) {
            order.push(title.clone());
        }
        let status = test
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                if test.get("passed").and_then(Value::as_bool).unwrap_or(false) {
                    "passed"
                } else {
                    "failed"
                }
                .to_string()
            });
        let duration = test
            .get("durationMs")
            .or_else(|| test.get("duration"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        by_title.insert(
            title.clone(),
            json!({ "title": title, "status": status, "durationMs": js_number(duration) }),
        );
    }
    order
        .into_iter()
        .filter_map(|title| by_title.remove(&title))
        .collect()
}

pub(crate) fn js_number(value: f64) -> Value {
    if !value.is_finite() {
        Value::Null
    } else if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        json!(value as i64)
    } else {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

