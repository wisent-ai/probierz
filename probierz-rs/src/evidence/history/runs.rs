use crate::evidence::*;
use serde_json::json;

/// The schema of the run comparison report.
const COMPARISON_SCHEMA_VERSION: u64 = 2;
/// The record the run manifest at `manifest_path` becomes; `None` when the
/// manifest is gone. A manifest, analysis or report that is there but cannot
/// be read is the failure naming it, never a run left out of the history.
pub(crate) fn run_record(manifest_path: &Path) -> Result<Option<Value>, Failure> {
    let Some(source) = crate::json_file::read(manifest_path)? else {
        return Ok(None);
    };
    let Some(directory) = manifest_path.parent() else {
        return Ok(None);
    };
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
    let duration = match source.get("durationMs").and_then(Value::as_f64) {
        Some(duration) => crate::run::number(duration),
        None => Value::Null,
    };
    record.insert("durationMs".into(), duration);
    // What the manifest did not state stays null in the record.
    let stated = |field: &str| match source.get(field) {
        Some(value) => value.clone(),
        None => Value::Null,
    };
    record.insert("harness".into(), stated("harness"));
    record.insert("source".into(), stated("source"));
    record.insert("build".into(), stated("build"));
    record.insert(
        "journeys".into(),
        match source.pointer("/appManifest/journeys") {
            Some(journeys) => journeys.clone(),
            None => Value::Null,
        },
    );
    record.insert(
        "failureClass".into(),
        if record.get("status").and_then(Value::as_str) == Some("failed") {
            json!(crate::status::failure_class(&source))
        } else {
            Value::Null
        },
    );
    record.insert(
        "device".into(),
        source.get("device").cloned().unwrap_or(Value::Null),
    );
    record.insert("conditions".into(), stated("conditions"));
    record.insert(
        "evidence".into(),
        source.get("evidence").cloned().unwrap_or(Value::Null),
    );
    record.insert("artifacts".into(), stated("artifacts"));
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
    record.insert(
        "tests".into(),
        Value::Array(tests_from(directory, &source)?),
    );
    Ok(Some(Value::Object(record)))
}

pub(crate) fn get_run(harness: &Path, app_id: &str, run_id: &str) -> Result<Value, Failure> {
    for file in manifests_below(&harness.join("test-results").join(app_id))? {
        if let Some(run) = run_record(&file)? {
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
    app_id: &str,
) -> Answer {
    let left_id = left_id.ok_or_else(|| {
        Failure::invalid("evidence.compare", "compare needs left and right run IDs")
    })?;
    let right_id = right_id.ok_or_else(|| {
        Failure::invalid("evidence.compare", "compare needs left and right run IDs")
    })?;
    let left = get_run(harness, app_id, left_id)?;
    let right = get_run(harness, app_id, right_id)?;
    let tests = compare_named(&left, &right, "tests", "title", true);
    let artifacts = compare_named(&left, &right, "artifacts", "file", false);
    // A duration is compared only when both runs stated one; an unstated
    // one is never read as zero.
    let left_duration = left.get("durationMs").and_then(Value::as_f64);
    let right_duration = right.get("durationMs").and_then(Value::as_f64);
    let ratio = left_duration
        .zip(right_duration)
        .filter(|(left, _)| left.is_normal())
        .map(|(left, right)| right / left);
    let delta = left_duration
        .zip(right_duration)
        .map(|(left, right)| crate::run::number(right - left));
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
            "durationMs": run.get("durationMs"),
            "evidence": run.get("evidence").cloned().unwrap_or(Value::Null),
        })
    };
    print_json(&json!({
        "schemaVersion": COMPARISON_SCHEMA_VERSION,
        "appId": app_id,
        "left": side(&left),
        "right": side(&right),
        "verdict": {
            "statusChanged": left.get("status") != right.get("status"),
            "regression": right.get("status").and_then(Value::as_str) == Some("failed") && left.get("status").and_then(Value::as_str) == Some("passed"),
            "newlyFailing": newly_failing,
        },
        "duration": { "deltaMs": delta, "ratio": ratio.map(js_number) },
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
                let old_duration = old.get("durationMs").and_then(Value::as_f64);
                let new_duration = new.get("durationMs").and_then(Value::as_f64);
                if old_status != new_status || old_duration != new_duration {
                    changes.push(json!({
                        name: entry_name,
                        "change": if old_status == new_status { "duration" } else { "status" },
                        "before": old,
                        "after": new,
                        "durationDeltaMs": old_duration.zip(new_duration).map(|(old, new)| crate::run::number(new - old)),
                    }));
                }
            }
            (Some(old), Some(new))
                if old.get("sha256") != new.get("sha256")
                    || old.get("bytes") != new.get("bytes") =>
            {
                changes.push(
                    json!({ name: entry_name, "change": "content", "before": old, "after": new }),
                );
            }
            _ => {}
        }
    }
    (changes.len(), changes)
}
