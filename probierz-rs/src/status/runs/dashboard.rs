//! The dashboard over an application's runs, grouped by device and version.

use crate::status::*;

pub(crate) fn yaml_json(value: &serde_yaml::Value) -> Result<Value, Failure> {
    serde_json::to_value(value).map_err(|error| Failure::config("manifest.read", error.to_string()))
}

pub(crate) fn manifest_object(
    harness: &Path,
    app_id: &str,
) -> Result<(manifest::Manifest, Value), Failure> {
    let loaded = manifest::load(harness, app_id)?;
    let document = yaml_json(&loaded.document)?;
    Ok((loaded, document))
}

pub(crate) fn artifact_projection(run: &Value) -> Value {
    Value::Array(
        run.get("artifacts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|artifact| {
                json!({
                    "file": value_or(artifact.get("file"), Value::Null),
                    "sha256": value_or(artifact.get("sha256"), Value::Null),
                    "bytes": json_number(number(artifact.get("bytes"))),
                })
            })
            .collect(),
    )
}

pub(crate) fn result_projection(run: Option<&Value>) -> Value {
    let Some(run) = run else {
        return Value::Null;
    };
    json!({
        "runId": value_or(run.get("runId"), Value::Null),
        "status": value_or(run.get("status"), Value::Null),
        "startedAt": value_or(run.get("startedAt"), Value::Null),
        "completedAt": value_or(run.get("completedAt"), Value::Null),
        "durationMs": value_or(run.get("durationMs"), json!(0)),
        "evidence": value_or(run.get("evidence"), Value::Null),
        "artifacts": artifact_projection(run),
    })
}

pub(crate) fn device_key(run: &Value) -> String {
    let name = run
        .get("device")
        .and_then(|device| string(device.get("name")))
        .unwrap_or("host");
    let runtime = run
        .get("device")
        .and_then(|device| string(device.get("runtime")))
        .unwrap_or("default");
    format!("{name}:{runtime}")
}

pub(crate) fn version_key(run: &Value) -> String {
    run.get("build")
        .and_then(|value| string(value.get("sha256")))
        .or_else(|| {
            run.get("harness")
                .and_then(|value| string(value.get("sha256")))
        })
        .unwrap_or("unknown")
        .to_string()
}

pub(crate) fn dashboard_value(
    harness: &Path,
    app_id: &str,
    limit: usize,
) -> Result<Value, Failure> {
    let (loaded, document) = manifest_object(harness, app_id)?;
    let history = run_history_value(harness, app_id, None, limit)?;
    let runs = history
        .get("runs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut versions: Vec<(String, Vec<Value>, Value, Value, Value, Value)> = Vec::new();
    for run in runs {
        let key = version_key(&run);
        if let Some((_, grouped, _, _, _, _)) = versions.iter_mut().find(|entry| entry.0 == key) {
            grouped.push(run);
        } else {
            versions.push((
                key,
                vec![run.clone()],
                value_or(run.get("harness"), Value::Null),
                value_or(run.get("source"), Value::Null),
                value_or(run.get("build"), Value::Null),
                value_or(run.get("startedAt"), Value::Null),
            ));
        }
    }
    let journey_map = document
        .get("journeys")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let surface_map = document
        .get("surfaces")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut projected_versions = Vec::new();
    for (version, version_runs, harness_value, source, build, latest_at) in versions {
        let mut journeys = Vec::new();
        for (journey_id, journey) in &journey_map {
            let mut surfaces = Vec::new();
            for (target, surface) in &surface_map {
                let includes = surface
                    .get("journeys")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| item.as_str() == Some(journey_id))
                    });
                if !includes {
                    continue;
                }
                let relevant = version_runs
                    .iter()
                    .filter(|run| {
                        run.get("target").and_then(Value::as_str) == Some(target)
                            && run
                                .get("journeys")
                                .and_then(Value::as_array)
                                .is_some_and(|items| {
                                    items.iter().any(|item| item.as_str() == Some(journey_id))
                                })
                    })
                    .collect::<Vec<_>>();
                let mut devices: Vec<(String, Vec<&Value>)> = Vec::new();
                for run in &relevant {
                    let key = device_key(run);
                    if let Some((_, grouped)) = devices.iter_mut().find(|entry| entry.0 == key) {
                        grouped.push(*run);
                    } else {
                        devices.push((key, vec![*run]));
                    }
                }
                let mut projected_devices = devices
                    .into_iter()
                    .map(|(device, runs)| {
                        json!({
                            "device": device,
                            "status": runs.first().and_then(|run| run.get("status")).cloned().unwrap_or(Value::Null),
                            "latest": result_projection(runs.first().copied()),
                            "runs": runs.into_iter().map(|run| result_projection(Some(run))).collect::<Vec<_>>(),
                        })
                    })
                    .collect::<Vec<_>>();
                projected_devices.sort_by(|left, right| {
                    string(left.get("device"))
                        .unwrap_or("")
                        .cmp(string(right.get("device")).unwrap_or(""))
                });
                surfaces.push(json!({
                    "target": target,
                    "status": relevant.first().and_then(|run| run.get("status")).and_then(Value::as_str).unwrap_or("missing"),
                    "latest": result_projection(relevant.first().copied()),
                    "devices": projected_devices,
                }));
            }
            surfaces.sort_by(|left, right| {
                string(left.get("target"))
                    .unwrap_or("")
                    .cmp(string(right.get("target")).unwrap_or(""))
            });
            let statuses = surfaces
                .iter()
                .filter_map(|surface| surface.get("status").and_then(Value::as_str))
                .collect::<Vec<_>>();
            let status = if !statuses.is_empty() && statuses.iter().all(|value| *value == "passed")
            {
                "passed"
            } else if statuses.contains(&"failed") {
                "failed"
            } else {
                "incomplete"
            };
            journeys.push(json!({
                "journey": journey_id,
                "owner": value_or(journey.get("owner"), Value::Null),
                "status": status,
                "surfaces": surfaces,
            }));
        }
        journeys.sort_by(|left, right| {
            string(left.get("journey"))
                .unwrap_or("")
                .cmp(string(right.get("journey")).unwrap_or(""))
        });
        let status = if !journeys.is_empty()
            && journeys
                .iter()
                .all(|journey| journey.get("status").and_then(Value::as_str) == Some("passed"))
        {
            "passed"
        } else if journeys
            .iter()
            .any(|journey| journey.get("status").and_then(Value::as_str) == Some("failed"))
        {
            "failed"
        } else {
            "incomplete"
        };
        projected_versions.push(json!({
            "version": version,
            "harness": harness_value,
            "source": source,
            "build": build,
            "latestAt": latest_at,
            "status": status,
            "journeys": journeys,
        }));
    }
    let mut requirements = journey_map
        .iter()
        .map(|(journey, detail)| {
            let mut surfaces = surface_map
                .iter()
                .filter(|(_, surface)| {
                    surface
                        .get("journeys")
                        .and_then(Value::as_array)
                        .is_some_and(|items| {
                            items.iter().any(|item| item.as_str() == Some(journey))
                        })
                })
                .map(|(target, _)| Value::String(target.clone()))
                .collect::<Vec<_>>();
            surfaces.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
            json!({
                "journey": journey,
                "owner": value_or(detail.get("owner"), Value::Null),
                "surfaces": surfaces,
            })
        })
        .collect::<Vec<_>>();
    requirements.sort_by(|left, right| {
        string(left.get("journey"))
            .unwrap_or("")
            .cmp(string(right.get("journey")).unwrap_or(""))
    });
    Ok(json!({
        "schemaVersion": 2,
        "generatedAt": now(),
        "product": {
            "appId": app_id,
            "owner": value_or(document.get("owner"), Value::Null),
            "manifest": loaded.file.to_string_lossy(),
        },
        "requirements": requirements,
        "summary": {
            "versions": projected_versions.len(),
            "runs": value_or(history.pointer("/summary/runs"), json!(0)),
            "latestRunId": value_or(history.pointer("/summary/latestRunId"), Value::Null),
            "lastGreenRunId": value_or(history.pointer("/summary/lastGreenRunId"), Value::Null),
        },
        "versions": projected_versions,
    }))
}

pub fn dashboard(harness: &Path, app_id: &str, limit: usize) -> Answer {
    print_json(&dashboard_value(harness, app_id, limit)?)
}
