//! Run records: the manifests below a root, their normalised status and failure class, and the record one run manifest becomes.

use super::*;

pub(super) fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(super) fn json_number(value: f64) -> Value {
    if value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value <= i64::MAX as f64
    {
        Value::Number(Number::from(value as i64))
    } else {
        Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

pub(super) fn number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(value)) => value.as_f64().unwrap_or(0.0),
        Some(Value::String(value)) => value.parse::<f64>().unwrap_or(f64::NAN),
        Some(Value::Bool(value)) => usize::from(*value) as f64,
        Some(Value::Null) | None => 0.0,
        _ => f64::NAN,
    }
}

pub(super) fn string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

pub(super) fn manifests_below(root: &Path) -> Result<Vec<PathBuf>, Failure> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() && entry.file_name() == "run-manifest.json" {
                files.push(entry.path());
            }
        }
    }
    Ok(files)
}

pub(super) fn read_json(path: &Path) -> Option<Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

pub(super) fn normalized_status(manifest: &Value) -> &str {
    match manifest.get("status").and_then(Value::as_str) {
        Some("passed") => "passed",
        Some("blocked") => "blocked",
        Some("canceled") => "canceled",
        Some("executed") => "passed",
        Some("failed") => "failed",
        _ if manifest
            .get("completedAt")
            .is_some_and(|value| !value.is_null()) =>
        {
            "failed"
        }
        _ => "incomplete",
    }
}

pub(super) fn failure_class(analysis: Option<&Value>, report: Option<&Value>) -> &'static str {
    let failures = analysis
        .and_then(|value| value.get("failures"))
        .and_then(Value::as_array)
        .or_else(|| {
            report
                .and_then(|value| value.get("failures"))
                .and_then(Value::as_array)
        });
    let text = failures
        .into_iter()
        .flatten()
        .filter_map(|failure| {
            string(failure.get("error")).or_else(|| string(failure.get("message")))
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let driver_missing = text
        .find("driver")
        .and_then(|start| text[start..].find("not installed"))
        .is_some();
    if text.contains("executable doesn't exist")
        || driver_missing
        || text.contains("toolchain")
        || text.contains("connection refused")
        || text.contains("econnrefused")
    {
        "infrastructure"
    } else {
        "product"
    }
}

pub(super) fn value_or(value: Option<&Value>, fallback: Value) -> Value {
    match value {
        Some(Value::Null) | None => fallback,
        Some(value) => value.clone(),
    }
}

pub(super) fn run_record(manifest_path: &Path) -> Option<Value> {
    let manifest = read_json(manifest_path)?;
    let directory = manifest_path.parent()?;
    let analysis_path = string(manifest.get("analysisPath"))
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("analysis.json"));
    let report_path = manifest
        .get("paths")
        .and_then(|paths| string(paths.get("reportPath")))
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("report.json"));
    let analysis = read_json(&analysis_path);
    let report = read_json(&report_path);

    let source_tests = analysis
        .as_ref()
        .and_then(|value| value.get("tests"))
        .and_then(Value::as_array)
        .or_else(|| {
            report
                .as_ref()
                .and_then(|value| value.get("tests"))
                .and_then(Value::as_array)
        });
    let mut test_order = Vec::new();
    let mut tests_by_title: HashMap<String, Value> = HashMap::new();
    for test in source_tests.into_iter().flatten() {
        let Some(title) = test.get("title").and_then(Value::as_str) else {
            continue;
        };
        if !tests_by_title.contains_key(title) {
            test_order.push(title.to_string());
        }
        let status = string(test.get("status")).unwrap_or_else(|| {
            if test.get("passed").and_then(Value::as_bool).unwrap_or(false) {
                "passed"
            } else {
                "failed"
            }
        });
        let duration = if test.get("durationMs").is_some_and(|value| !value.is_null()) {
            number(test.get("durationMs"))
        } else if test.get("duration").is_some_and(|value| !value.is_null()) {
            number(test.get("duration"))
        } else {
            0.0
        };
        tests_by_title.insert(
            title.to_string(),
            json!({
                "title": title,
                "status": status,
                "durationMs": json_number(duration),
            }),
        );
    }
    let tests = test_order
        .iter()
        .filter_map(|title| tests_by_title.get(title).cloned())
        .collect::<Vec<_>>();
    let status = normalized_status(&manifest);
    let class = if status == "failed" {
        Value::String(failure_class(analysis.as_ref(), report.as_ref()).to_string())
    } else {
        Value::Null
    };
    let journeys = manifest
        .get("appManifest")
        .and_then(|value| value.get("journeys"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    Some(json!({
        "runId": value_or(manifest.get("runId"), Value::Null),
        "appId": value_or(manifest.get("appId"), Value::Null),
        "kind": string(manifest.get("kind")).unwrap_or("adhoc"),
        "target": value_or(manifest.get("target"), Value::Null),
        "spec": value_or(manifest.get("spec"), Value::Null),
        "status": status,
        "startedAt": value_or(manifest.get("startedAt"), Value::Null),
        "completedAt": value_or(manifest.get("completedAt"), Value::Null),
        "durationMs": json_number(number(manifest.get("durationMs"))),
        "harness": value_or(manifest.get("harness"), Value::Null),
        "source": value_or(manifest.get("source"), Value::Null),
        "build": value_or(manifest.get("build"), Value::Null),
        "journeys": journeys,
        "failureClass": class,
        "device": value_or(manifest.get("device"), Value::Null),
        "conditions": value_or(manifest.get("conditions"), json!({})),
        "evidence": value_or(manifest.get("evidence"), Value::Null),
        "artifacts": value_or(manifest.get("artifacts"), json!([])),
        "protection": value_or(manifest.get("protection"), Value::Null),
        "manifestPath": manifest_path.to_string_lossy(),
        "analysisPath": value_or(manifest.get("analysisPath"), Value::Null),
        "tests": tests,
    }))
}
