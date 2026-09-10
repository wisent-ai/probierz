use serde_json::json;
use crate::evidence::*;
pub(crate) fn run_record(manifest_path: &Path) -> Option<Value> {
    let source = try_json_file(manifest_path)?;
    let directory = manifest_path.parent()?;
    let analysis_path = source
        .get("analysisPath")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("analysis.json"));
    let report_path = source
        .pointer("/paths/reportPath")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("report.json"));
    let analysis = try_json_file(&analysis_path);
    let report = try_json_file(&report_path);
    let failures = analysis
        .as_ref()
        .and_then(|value| value.get("failures"))
        .and_then(Value::as_array)
        .or_else(|| {
            report
                .as_ref()
                .and_then(|value| value.get("failures"))
                .and_then(Value::as_array)
        });
    let failure_text = failures
        .into_iter()
        .flatten()
        .map(|failure| {
            failure
                .get("error")
                .or_else(|| failure.get("message"))
                .and_then(Value::as_str)
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let infrastructure = [
        "executable doesn't exist",
        "toolchain",
        "connection refused",
        "econnrefused",
    ]
    .iter()
    .any(|needle| failure_text.contains(needle))
        || (failure_text.contains("driver") && failure_text.contains("not installed"));
    let status = normalized_status(&source);
    let mut record = Map::new();
    record.insert(
        "runId".into(),
        source.get("runId").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "appId".into(),
        source.get("appId").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "kind".into(),
        source
            .get("kind")
            .cloned()
            .unwrap_or_else(|| json!("adhoc")),
    );
    record.insert(
        "target".into(),
        source.get("target").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "spec".into(),
        source.get("spec").cloned().unwrap_or(Value::Null),
    );
    record.insert("status".into(), json!(status));
    record.insert(
        "startedAt".into(),
        source.get("startedAt").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "completedAt".into(),
        source.get("completedAt").cloned().unwrap_or(Value::Null),
    );
    let duration = source
        .get("durationMs")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    record.insert("durationMs".into(), js_number(duration));
    for key in ["harness", "source", "build"] {
        record.insert(key.into(), source.get(key).cloned().unwrap_or(Value::Null));
    }
    record.insert(
        "journeys".into(),
        source
            .pointer("/appManifest/journeys")
            .cloned()
            .unwrap_or_else(|| json!([])),
    );
    record.insert(
        "failureClass".into(),
        if record.get("status").and_then(Value::as_str) == Some("failed") {
            json!(if infrastructure {
                "infrastructure"
            } else {
                "product"
            })
        } else {
            Value::Null
        },
    );
    record.insert(
        "device".into(),
        source.get("device").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "conditions".into(),
        source
            .get("conditions")
            .cloned()
            .unwrap_or_else(|| json!({})),
    );
    record.insert(
        "evidence".into(),
        source.get("evidence").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "artifacts".into(),
        source
            .get("artifacts")
            .cloned()
            .unwrap_or_else(|| json!([])),
    );
    record.insert(
        "protection".into(),
        source.get("protection").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "manifestPath".into(),
        json!(manifest_path.to_string_lossy()),
    );
    record.insert(
        "analysisPath".into(),
        source.get("analysisPath").cloned().unwrap_or(Value::Null),
    );
    record.insert("tests".into(), Value::Array(tests_from(directory, &source)));
    Some(Value::Object(record))
}

pub(crate) fn get_run(harness: &Path, app_id: &str, run_id: &str) -> Result<Value, Failure> {
    for file in manifests_below(&harness.join("test-results").join(app_id))? {
        if let Some(run) = run_record(&file) {
            if run.get("runId").and_then(Value::as_str) == Some(run_id) {
                return Ok(run);
            }
        }
    }
    Err(Failure::invalid(
        "evidence.run",
        format!("run not found for {app_id}: {run_id}"),
    ))
}

pub fn compare(
    harness: &Path,
    left_id: Option<&str>,
    right_id: Option<&str>,
    app_id: Option<&str>,
) -> Answer {
    let left_id = left_id.ok_or_else(|| {
        Failure::invalid("evidence.compare", "compare needs left and right run IDs")
    })?;
    let right_id = right_id.ok_or_else(|| {
        Failure::invalid("evidence.compare", "compare needs left and right run IDs")
    })?;
    let app_id = app_id.unwrap_or("probierz");
    let left = get_run(harness, app_id, left_id)?;
    let right = get_run(harness, app_id, right_id)?;
    let tests = compare_named(&left, &right, "tests", "title", true);
    let artifacts = compare_named(&left, &right, "artifacts", "file", false);
    let left_duration = left
        .get("durationMs")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let right_duration = right
        .get("durationMs")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let ratio = if left_duration > 0.0 {
        Some(right_duration / left_duration)
    } else {
        None
    };
    let newly_failing = tests
        .1
        .iter()
        .filter(|change| {
            change.pointer("/after/status").and_then(Value::as_str) == Some("failed")
                && change.pointer("/before/status").and_then(Value::as_str) != Some("failed")
        })
        .filter_map(|change| change.get("title").cloned())
        .collect::<Vec<_>>();
    let side = |run: &Value| {
        json!({
            "runId": run.get("runId").cloned().unwrap_or(Value::Null),
            "status": run.get("status").cloned().unwrap_or(Value::Null),
            "harness": run.get("harness").cloned().unwrap_or(Value::Null),
            "source": run.get("source").cloned().unwrap_or(Value::Null),
            "build": run.get("build").cloned().unwrap_or(Value::Null),
            "durationMs": run.get("durationMs").cloned().unwrap_or_else(|| json!(0)),
            "evidence": run.get("evidence").cloned().unwrap_or(Value::Null),
        })
    };
    print_json(&json!({
        "schemaVersion": 2,
        "appId": app_id,
        "left": side(&left),
        "right": side(&right),
        "verdict": {
            "statusChanged": left.get("status") != right.get("status"),
            "regression": right.get("status").and_then(Value::as_str) == Some("failed") && left.get("status").and_then(Value::as_str) == Some("passed"),
            "newlyFailing": newly_failing,
            "durationRegression": ratio.is_some_and(|value| left_duration >= 500.0 && value >= 1.2),
        },
        "duration": { "deltaMs": js_number(right_duration - left_duration), "ratio": ratio.map(js_number).unwrap_or(Value::Null) },
        "tests": { "changed": tests.0, "changes": tests.1 },
        "artifacts": { "changed": artifacts.0, "changes": artifacts.1 },
    }))
}

pub(crate) fn compare_named(
    left: &Value,
    right: &Value,
    collection: &str,
    name: &str,
    test: bool,
) -> (usize, Vec<Value>) {
    let indexed = |run: &Value| -> HashMap<String, Value> {
        run.get(collection)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                item.get(name)
                    .and_then(Value::as_str)
                    .map(|key| (key.to_string(), item.clone()))
            })
            .collect()
    };
    let before = indexed(left);
    let after = indexed(right);
    let mut names: BTreeSet<String> = before.keys().cloned().collect();
    names.extend(after.keys().cloned());
    let mut changes = Vec::new();
    for entry_name in names {
        match (before.get(&entry_name), after.get(&entry_name)) {
            (None, Some(value)) => changes.push(
                json!({ name: entry_name, "change": "added", "before": null, "after": value }),
            ),
            (Some(value), None) => changes.push(
                json!({ name: entry_name, "change": "removed", "before": value, "after": null }),
            ),
            (Some(old), Some(new)) if test => {
                let old_status = old.get("status");
                let new_status = new.get("status");
                let old_duration = old.get("durationMs").and_then(Value::as_f64).unwrap_or(0.0);
                let new_duration = new.get("durationMs").and_then(Value::as_f64).unwrap_or(0.0);
                if old_status != new_status || old_duration != new_duration {
                    changes.push(json!({
                        name: entry_name,
                        "change": if old_status == new_status { "duration" } else { "status" },
                        "before": old,
                        "after": new,
                        "durationDeltaMs": js_number(new_duration - old_duration),
                    }));
                }
            }
            (Some(old), Some(new)) => {
                if old.get("sha256") != new.get("sha256") || old.get("bytes") != new.get("bytes") {
                    changes.push(json!({ name: entry_name, "change": "content", "before": old, "after": new }));
                }
            }
            _ => {}
        }
    }
    (changes.len(), changes)
}

