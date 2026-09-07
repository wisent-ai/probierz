//! Run history, operator status, dashboards, and desktop failure intake.
//!
//! These are read surfaces over manifests and immutable run manifests. The
//! intake listener is the one write surface here: it appends bounded JSON lines
//! outside TCC-protected project directories so desktop applications and this
//! CLI share one store.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use base64::Engine;
use chrono::{SecondsFormat, Utc};
use rand_core::RngCore;
use serde_json::{json, Number, Value};

use crate::failure::{print_json, Answer, Code, Failure};
use crate::manifest;

const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
const DEFAULT_BIND: &str = "127.0.0.1:9790";
const ERROR_CODES: [&str; 7] = [
    "config",
    "auth",
    "not_found",
    "rate_limit",
    "timeout",
    "infra_down",
    "unknown",
];

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn json_number(value: f64) -> Value {
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

fn number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(value)) => value.as_f64().unwrap_or(0.0),
        Some(Value::String(value)) => value.parse::<f64>().unwrap_or(f64::NAN),
        Some(Value::Bool(value)) => usize::from(*value) as f64,
        Some(Value::Null) | None => 0.0,
        _ => f64::NAN,
    }
}

fn string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn manifests_below(root: &Path) -> Result<Vec<PathBuf>, Failure> {
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

fn read_json(path: &Path) -> Option<Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

fn normalized_status(manifest: &Value) -> &str {
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

fn failure_class(analysis: Option<&Value>, report: Option<&Value>) -> &'static str {
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

fn value_or(value: Option<&Value>, fallback: Value) -> Value {
    match value {
        Some(Value::Null) | None => fallback,
        Some(value) => value.clone(),
    }
}

fn run_record(manifest_path: &Path) -> Option<Value> {
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

fn percentile(mut values: Vec<f64>, fraction: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let index = ((values.len() as f64 * fraction).ceil() as usize).saturating_sub(1);
    values.get(index.min(values.len() - 1)).copied()
}

fn test_history(runs: &[Value]) -> Vec<Value> {
    let mut order = Vec::new();
    let mut by_title: HashMap<String, Vec<Value>> = HashMap::new();
    for run in runs.iter().rev() {
        if run.get("failureClass").and_then(Value::as_str) == Some("infrastructure") {
            continue;
        }
        for test in run
            .get("tests")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(title) = test.get("title").and_then(Value::as_str) else {
                continue;
            };
            if !by_title.contains_key(title) {
                order.push(title.to_string());
            }
            by_title.entry(title.to_string()).or_default().push(json!({
                "runId": value_or(run.get("runId"), Value::Null),
                "at": value_or(run.get("startedAt"), Value::Null),
                "status": value_or(test.get("status"), Value::Null),
                "durationMs": value_or(test.get("durationMs"), json!(0)),
            }));
        }
    }
    order.sort();
    order
        .into_iter()
        .map(|title| {
            let observations = by_title.remove(&title).unwrap_or_default();
            let passed = observations
                .iter()
                .filter(|item| item.get("status").and_then(Value::as_str) == Some("passed"))
                .count();
            let transitions = observations
                .windows(2)
                .filter(|window| window[0].get("status") != window[1].get("status"))
                .count();
            let durations = observations
                .iter()
                .filter(|item| item.get("status").and_then(Value::as_str) == Some("passed"))
                .map(|item| number(item.get("durationMs")))
                .collect::<Vec<_>>();
            let p50 = percentile(durations.clone(), 0.5).map(json_number).unwrap_or(Value::Null);
            let p95 = percentile(durations.clone(), 0.95).map(json_number).unwrap_or(Value::Null);
            let max = durations
                .iter()
                .copied()
                .reduce(f64::max)
                .map(json_number)
                .unwrap_or(Value::Null);
            let total = observations.len();
            json!({
                "title": title,
                "observations": total,
                "passed": passed,
                "failed": total - passed,
                "passRate": if total > 0 { json_number(passed as f64 / total as f64) } else { Value::Null },
                "transitions": transitions,
                "flaky": transitions > 0 && passed > 0 && passed < total,
                "latest": observations.last().cloned().unwrap_or(Value::Null),
                "duration": { "p50Ms": p50, "p95Ms": p95, "maxMs": max },
            })
        })
        .collect()
}

fn journey_history(runs: &[Value]) -> Vec<Value> {
    let names = runs
        .iter()
        .flat_map(|run| {
            run.get("journeys")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    names
        .into_iter()
        .map(|journey| {
            let relevant = runs
                .iter()
                .filter(|run| {
                    run.get("journeys")
                        .and_then(Value::as_array)
                        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(&journey)))
                })
                .collect::<Vec<_>>();
            let count_status = |wanted: &str| {
                relevant
                    .iter()
                    .filter(|run| run.get("status").and_then(Value::as_str) == Some(wanted))
                    .count()
            };
            let passed = count_status("passed");
            let product_failures = relevant
                .iter()
                .filter(|run| {
                    run.get("status").and_then(Value::as_str) == Some("failed")
                        && run.get("failureClass").and_then(Value::as_str) != Some("infrastructure")
                })
                .count();
            let infrastructure_failures = relevant
                .iter()
                .filter(|run| {
                    run.get("status").and_then(Value::as_str) == Some("failed")
                        && run.get("failureClass").and_then(Value::as_str) == Some("infrastructure")
                })
                .count();
            json!({
                "journey": journey,
                "runs": relevant.len(),
                "passed": passed,
                "failed": count_status("failed"),
                "productFailures": product_failures,
                "infrastructureFailures": infrastructure_failures,
                "blocked": count_status("blocked"),
                "canceled": count_status("canceled"),
                "passRate": if passed + product_failures > 0 {
                    json_number(passed as f64 / (passed + product_failures) as f64)
                } else { Value::Null },
                "latestRunId": relevant.first().and_then(|run| run.get("runId")).cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

fn performance_trend(runs: &[Value]) -> Value {
    let passed = runs
        .iter()
        .filter(|run| {
            run.get("status").and_then(Value::as_str) == Some("passed")
                && number(run.get("durationMs")) > 0.0
        })
        .collect::<Vec<_>>();
    let latest_ms = passed.first().map(|run| number(run.get("durationMs")));
    let baseline = percentile(
        passed
            .iter()
            .skip(1)
            .take(10)
            .map(|run| number(run.get("durationMs")))
            .collect(),
        0.5,
    );
    let ratio = latest_ms.zip(baseline).and_then(|(latest, baseline)| {
        if baseline != 0.0 {
            Some(latest / baseline)
        } else {
            None
        }
    });
    json!({
        "latestMs": latest_ms.map(json_number).unwrap_or(Value::Null),
        "baselineMedianMs": baseline.map(json_number).unwrap_or(Value::Null),
        "ratio": ratio.map(json_number).unwrap_or(Value::Null),
        "regression": ratio.is_some_and(|ratio| baseline.unwrap_or(0.0) >= 500.0 && ratio >= 1.2),
    })
}

pub(crate) fn run_history_value(
    harness: &Path,
    app_id: &str,
    target: Option<&str>,
    limit: usize,
) -> Result<Value, Failure> {
    let mut root = harness.join("test-results").join(app_id);
    if let Some(target) = target {
        root.push(target.replace(':', "-"));
    }
    let mut runs = manifests_below(&root)?
        .iter()
        .filter_map(|path| run_record(path))
        .filter(|run| {
            target.is_none_or(|target| run.get("target").and_then(Value::as_str) == Some(target))
        })
        .collect::<Vec<_>>();
    runs.sort_by(|left, right| {
        let left = left
            .get("startedAt")
            .map(Value::to_string)
            .unwrap_or_else(|| "undefined".to_string());
        let right = right
            .get("startedAt")
            .map(Value::to_string)
            .unwrap_or_else(|| "undefined".to_string());
        right.cmp(&left)
    });
    runs.truncate(limit.max(1));
    let count_status = |wanted: &str| {
        runs.iter()
            .filter(|run| run.get("status").and_then(Value::as_str) == Some(wanted))
            .count()
    };
    let passed = count_status("passed");
    let failed = count_status("failed");
    let infrastructure_failures = runs
        .iter()
        .filter(|run| {
            run.get("status").and_then(Value::as_str) == Some("failed")
                && run.get("failureClass").and_then(Value::as_str) == Some("infrastructure")
        })
        .count();
    let product_failures = failed - infrastructure_failures;
    let tests = test_history(&runs);
    let performance = performance_trend(&runs);
    Ok(json!({
        "schemaVersion": 2,
        "appId": app_id,
        "target": target,
        "generatedAt": now(),
        "summary": {
            "runs": runs.len(),
            "passed": passed,
            "failed": failed,
            "blocked": count_status("blocked"),
            "canceled": count_status("canceled"),
            "productFailures": product_failures,
            "infrastructureFailures": infrastructure_failures,
            "passRate": if passed + product_failures > 0 {
                json_number(passed as f64 / (passed + product_failures) as f64)
            } else { Value::Null },
            "flakyTests": tests.iter().filter(|test| test.get("flaky").and_then(Value::as_bool) == Some(true)).count(),
            "latestRunId": runs.first().and_then(|run| run.get("runId")).cloned().unwrap_or(Value::Null),
            "lastGreenRunId": runs.iter().find(|run| run.get("status").and_then(Value::as_str) == Some("passed"))
                .and_then(|run| run.get("runId")).cloned().unwrap_or(Value::Null),
            "performanceRegression": performance.get("regression").cloned().unwrap_or(Value::Bool(false)),
        },
        "performance": performance,
        "journeys": journey_history(&runs),
        "tests": tests,
        "runs": runs,
    }))
}

pub fn history(harness: &Path, app_id: &str, target: Option<&str>, limit: usize) -> Answer {
    print_json(&run_history_value(harness, app_id, target, limit)?)
}

fn yaml_json(value: &serde_yaml::Value) -> Result<Value, Failure> {
    serde_json::to_value(value).map_err(|error| Failure::config("manifest.read", error.to_string()))
}

fn manifest_object(harness: &Path, app_id: &str) -> Result<(manifest::Manifest, Value), Failure> {
    let loaded = manifest::load(harness, app_id)?;
    let document = yaml_json(&loaded.document)?;
    Ok((loaded, document))
}

fn artifact_projection(run: &Value) -> Value {
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

fn result_projection(run: Option<&Value>) -> Value {
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

fn device_key(run: &Value) -> String {
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

fn version_key(run: &Value) -> String {
    run.get("build")
        .and_then(|value| string(value.get("sha256")))
        .or_else(|| {
            run.get("harness")
                .and_then(|value| string(value.get("sha256")))
        })
        .unwrap_or("unknown")
        .to_string()
}

fn dashboard_value(harness: &Path, app_id: &str, limit: usize) -> Result<Value, Failure> {
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

fn git(root: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn git_lines(root: &str, arguments: &[&str]) -> Vec<String> {
    let Some(output) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .ok()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn glob_matches(pattern: &str, text: &str) -> bool {
    fn matches(
        pattern: &[u8],
        text: &[u8],
        memo: &mut HashMap<(usize, usize), bool>,
        pi: usize,
        ti: usize,
    ) -> bool {
        if let Some(answer) = memo.get(&(pi, ti)) {
            return *answer;
        }
        let answer = if pi == pattern.len() {
            ti == text.len()
        } else if pattern[pi] == b'*' && pi + 1 < pattern.len() && pattern[pi + 1] == b'*' {
            matches(pattern, text, memo, pi + 2, ti)
                || (ti < text.len() && matches(pattern, text, memo, pi, ti + 1))
        } else if pattern[pi] == b'*' {
            matches(pattern, text, memo, pi + 1, ti)
                || (ti < text.len() && text[ti] != b'/' && matches(pattern, text, memo, pi, ti + 1))
        } else {
            ti < text.len()
                && pattern[pi] == text[ti]
                && matches(pattern, text, memo, pi + 1, ti + 1)
        };
        memo.insert((pi, ti), answer);
        answer
    }
    matches(
        pattern.as_bytes(),
        text.as_bytes(),
        &mut HashMap::new(),
        0,
        0,
    )
}

fn affected_journeys(harness: &Path, files: &[PathBuf]) -> Result<Vec<String>, Failure> {
    let mut affected = BTreeSet::new();
    for app in manifest::list(harness)? {
        let (_, document) = manifest_object(harness, &app.app_id)?;
        for repository in document
            .get("repositories")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(root) = repository.get("root").and_then(Value::as_str) else {
                continue;
            };
            let root_path = Path::new(root);
            for file in files {
                let Ok(relative) = file.strip_prefix(root_path) else {
                    continue;
                };
                let relative = relative
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                for mapping in repository
                    .get("mappings")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let matched = mapping
                        .get("paths")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .any(|pattern| glob_matches(pattern, &relative));
                    if matched {
                        for journey in mapping
                            .get("journeys")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                        {
                            if let Some(journey) = journey.as_str() {
                                affected.insert(journey.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(affected.into_iter().collect())
}

fn gate_status(loaded: &manifest::Manifest, app_id: &str) -> Result<Value, Failure> {
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

fn evidence_level(run: &Value) -> &'static str {
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

fn evidence_rank(level: &str) -> i32 {
    match level {
        "E0" => 0,
        "E1" => 1,
        "E2" => 2,
        "E3" => 3,
        _ => -1,
    }
}

fn app_status_value(harness: &Path, app_id: &str, base_ref: &str) -> Result<Value, Failure> {
    let (loaded, document) = manifest_object(harness, app_id)?;
    let history = run_history_value(harness, app_id, None, 1000)?;
    let gates = gate_status(&loaded, app_id)?;
    let mut repositories = Vec::new();
    let mut diff_files = Vec::new();
    for repository in document
        .get("repositories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let root = repository.get("root").and_then(Value::as_str).unwrap_or("");
        let head = git(root, &["rev-parse", "HEAD"]);
        let base = git(root, &["rev-parse", "--verify", base_ref]);
        if let (Some(head), Some(base)) = (&head, &base) {
            let range = format!("{base}..{head}");
            diff_files.extend(
                git_lines(root, &["diff", "--name-only", &range])
                    .into_iter()
                    .map(|file| Path::new(root).join(file)),
            );
        }
        repositories.push(json!({ "root": root, "headSha": head, "baseSha": base }));
    }
    let affected = affected_journeys(harness, &diff_files)?;
    let runs = history
        .get("runs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut latest: HashMap<String, Value> = HashMap::new();
    for run in &runs {
        for journey in run
            .get("journeys")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(journey) = journey.as_str() {
                latest
                    .entry(journey.to_string())
                    .or_insert_with(|| run.clone());
            }
        }
    }
    let mut journey_names = document
        .get("journeys")
        .and_then(Value::as_object)
        .map(|map| map.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    journey_names.sort();
    let mut journeys = Vec::new();
    for journey in journey_names {
        let run = latest.get(&journey);
        let fresh = run.is_some_and(|run| {
            repositories.iter().all(|repository| {
                let root = repository.get("root").and_then(Value::as_str).unwrap_or("");
                let recorded = run
                    .pointer("/source/repositories")
                    .and_then(Value::as_array)
                    .and_then(|recorded| {
                        recorded.iter().find(|item| {
                            string(item.get("name"))
                                .or_else(|| string(item.get("root")))
                                .is_some_and(|name| root.ends_with(name) || name == root)
                        })
                    });
                recorded.and_then(|item| item.get("gitSha")) == repository.get("headSha")
            })
        });
        let last_run = run
            .map(|run| {
                json!({
                    "runId": value_or(run.get("runId"), Value::Null),
                    "target": value_or(run.get("target"), Value::Null),
                    "status": value_or(run.get("status"), Value::Null),
                    "startedAt": value_or(run.get("startedAt"), Value::Null),
                    "evidenceLevel": evidence_level(run),
                })
            })
            .unwrap_or(Value::Null);
        journeys.push(json!({
            "journey": journey,
            "lastRun": last_run,
            "fresh": fresh,
            "affected": affected.iter().any(|name| name == &journey),
        }));
    }
    let minimum = document
        .pointer("/pullRequestPolicy/minimumEvidence")
        .and_then(Value::as_str)
        .unwrap_or("E2");
    let evaluated = journeys
        .iter()
        .filter(|journey| journey.get("affected").and_then(Value::as_bool) == Some(true))
        .collect::<Vec<_>>();
    let mut blocking = Vec::new();
    for journey in &evaluated {
        let name = journey.get("journey").and_then(Value::as_str).unwrap_or("");
        let last = journey.get("lastRun").filter(|value| !value.is_null());
        if last.is_none() {
            blocking.push(Value::String(format!("{name}: no runs recorded")));
            continue;
        }
        let last = last.unwrap_or(&Value::Null);
        let status = last.get("status").and_then(Value::as_str).unwrap_or("");
        if status != "passed" {
            blocking.push(Value::String(format!("{name}: last run is {status}")));
        }
        if journey.get("fresh").and_then(Value::as_bool) != Some(true) {
            blocking.push(Value::String(format!(
                "{name}: evidence is older than HEAD"
            )));
        }
        let level = last
            .get("evidenceLevel")
            .and_then(Value::as_str)
            .unwrap_or("E0");
        if evidence_rank(level) < evidence_rank(minimum) {
            blocking.push(Value::String(format!("{name}: {level} is below {minimum}")));
        }
    }
    let untested = journeys
        .iter()
        .filter(|journey| journey.get("lastRun").is_none_or(Value::is_null))
        .filter_map(|journey| journey.get("journey").cloned())
        .collect::<Vec<_>>();
    let evaluated_names = evaluated
        .iter()
        .filter_map(|journey| journey.get("journey").cloned())
        .collect::<Vec<_>>();
    let gate = gates
        .pointer("/modes/pull-request")
        .cloned()
        .unwrap_or(Value::Null);
    Ok(json!({
        "schemaVersion": 1,
        "appId": app_id,
        "generatedAt": now(),
        "baseRef": base_ref,
        "repositories": repositories,
        "journeys": journeys,
        "untested": untested,
        "affectedJourneys": affected,
        "mergeEligibility": {
            "mode": "pull-request",
            "minimumEvidence": minimum,
            "evaluatedJourneys": evaluated_names,
            "blockingReasons": blocking,
            "eligible": blocking.is_empty(),
            "gate": gate,
        },
    }))
}

fn render_app_status(status: &Value) -> String {
    let mut lines = vec![format!(
        "app: {}",
        status.get("appId").and_then(Value::as_str).unwrap_or("")
    )];
    lines.push("  journeys:".to_string());
    for journey in status
        .get("journeys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = journey.get("journey").and_then(Value::as_str).unwrap_or("");
        let last = journey.get("lastRun").filter(|value| !value.is_null());
        if last.is_none() {
            lines.push(format!("    {name:<24} — no runs —  E0  untested"));
            continue;
        }
        let run = last.unwrap_or(&Value::Null);
        let started = run.get("startedAt").and_then(Value::as_str).unwrap_or("");
        let date = started.get(..10.min(started.len())).unwrap_or(started);
        lines.push(format!(
            "    {name:<24} {date}  {}  {}  {}",
            run.get("runId").and_then(Value::as_str).unwrap_or(""),
            run.get("evidenceLevel")
                .and_then(Value::as_str)
                .unwrap_or(""),
            if journey.get("fresh").and_then(Value::as_bool) == Some(true) {
                "fresh"
            } else {
                "stale"
            },
        ));
    }
    let affected = status
        .get("affectedJourneys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let eligibility = status.get("mergeEligibility").unwrap_or(&Value::Null);
    lines.push(format!(
        "  affected ({}..HEAD): {}",
        status.get("baseRef").and_then(Value::as_str).unwrap_or(""),
        if affected.is_empty() {
            "(none)"
        } else {
            &affected
        },
    ));
    lines.push(format!(
        "  merge-eligibility({}, min {}): {}",
        eligibility
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or(""),
        eligibility
            .get("minimumEvidence")
            .and_then(Value::as_str)
            .unwrap_or(""),
        if eligibility.get("eligible").and_then(Value::as_bool) == Some(true) {
            "ELIGIBLE"
        } else {
            "BLOCKED"
        },
    ));
    for reason in eligibility
        .get("blockingReasons")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        lines.push(format!("    - {reason}"));
    }
    lines.join("\n")
}

pub fn status(harness: &Path, app_id: &str, base_ref: &str, text: bool) -> Result<bool, Failure> {
    let report = app_status_value(harness, app_id, base_ref)?;
    let eligible = report
        .pointer("/mergeEligibility/eligible")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if text {
        println!("{}", render_app_status(&report));
    } else {
        print_json(&report)?;
    }
    Ok(eligible)
}

fn violations_for(root: &str) -> Value {
    let output = Command::new("tama")
        .args(["find-violations", "--repo", root, "--json"])
        .output();
    let Ok(output) = output else {
        return json!({ "error": "exit null" });
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && !stdout.trim().starts_with('{') {
        let detail = stderr
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                format!(
                    "exit {}",
                    output
                        .status
                        .code()
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "null".to_string())
                )
            });
        return json!({ "error": detail });
    }
    let Ok(report) = serde_json::from_str::<Value>(&stdout) else {
        return json!({ "error": "scanner output not parseable" });
    };
    let repo = report.pointer("/repos/0").unwrap_or(&report);
    json!({
        "violations": repo.get("violations").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
        "skipped": repo.get("skippedFiles").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
        "errors": repo.get("errors").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
    })
}

fn fleet_failure(point: &str, code: &str, detail: &str, message: &str) -> Value {
    let (severity, retryable, outage) = code_meaning(code);
    let detail = trim_detail(detail, 300);
    eprintln!(
        "probierz-failure {}",
        json!({
            "failure_point": point,
            "error_code": code,
            "service": "objects",
            "impact": "fleet-health",
            "severity": severity,
            "retryable": retryable,
            "outage": outage,
            "detail": detail,
        })
    );
    fleet_summary(point, code, message)
}

fn fleet_summary(point: &str, code: &str, message: &str) -> Value {
    let (_, retryable, outage) = code_meaning(code);
    json!({
        "available": false,
        "failurePoint": point,
        "errorCode": code,
        "service": "objects",
        "retryable": retryable,
        "outage": outage,
        "message": message,
    })
}

fn object_message(action: &str, code: &str) -> String {
    let blame = match code {
        "infra_down" => format!("{action}: the objects dependency is unavailable. This is an infrastructure outage, not your configuration — retry later."),
        "timeout" => format!("{action}: the objects dependency did not answer in time. Not your configuration — retry later."),
        "rate_limit" => format!("{action}: the objects dependency is rate-limiting us. Retry later."),
        "config" => format!("{action}: the objects dependency is missing configuration. See the detail on the line above; retrying will not help."),
        "auth" => format!("{action}: the objects dependency rejected our credentials. Refresh them; retrying will not help."),
        "not_found" => format!("{action}: the objects dependency has no such object. Check the identifier; retrying will not help."),
        _ => format!("{action}: the objects dependency failed in a way probierz does not recognise. See the detail on the line above."),
    };
    if matches!(code, "infra_down" | "timeout" | "rate_limit") {
        format!("{blame} Local runs are unaffected — `probierz run <target>` still works without the stado queue.")
    } else {
        blame
    }
}

fn fleet_health() -> Value {
    let objects = match crate::evidence::list_objects("stado://probierz/capacity/") {
        Ok(objects) => objects,
        Err(failure) => {
            let code = match failure.code {
                Code::Config => "config",
                Code::Unavailable => "infra_down",
                Code::Invalid => "unknown",
                Code::Prerequisite => "config",
                Code::Refused => "unknown",
                Code::Unknown => "unknown",
            };
            let action = match failure.point.as_str() {
                "objects.config" => "Stado object storage is unusable",
                "objects.read" if failure.detail.contains("rejected") => {
                    "Stado object storage rejected the request"
                }
                "objects.read" => "Stado object storage did not answer",
                _ => "objects.list failed",
            };
            let message = object_message(action, code);
            return if matches!(failure.point.as_str(), "objects.config" | "objects.read") {
                fleet_failure(&failure.point, code, &failure.detail, &message)
            } else {
                fleet_summary("objects.list", code, &message)
            };
        }
    };
    let epoch = Utc::now().timestamp_millis();
    let mut agents = objects
        .iter()
        .filter_map(|object| {
            let updated = object.get("updated_at").and_then(Value::as_str)?;
            let updated = chrono::DateTime::parse_from_rfc3339(updated)
                .ok()?
                .with_timezone(&Utc);
            let name = object
                .get("key")
                .and_then(Value::as_str)?
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())?;
            Some((
                name.to_string(),
                updated.to_rfc3339_opts(SecondsFormat::Millis, true),
                epoch - updated.timestamp_millis() <= 900_000,
            ))
        })
        .collect::<Vec<_>>();
    agents.sort_by(|left, right| left.0.cmp(&right.0));
    json!({
        "available": true,
        "live": agents.iter().filter(|agent| agent.2).map(|agent| Value::String(agent.0.clone())).collect::<Vec<_>>(),
        "stale": agents.iter().filter(|agent| !agent.2).map(|agent| Value::String(agent.0.clone())).collect::<Vec<_>>(),
    })
}

fn overview_value(
    harness: &Path,
    app_ids: Option<&[String]>,
    include_violations: bool,
) -> Result<Value, Failure> {
    let ids = match app_ids {
        Some(ids) => ids.to_vec(),
        None => manifest::list(harness)?
            .into_iter()
            .map(|app| app.app_id)
            .collect(),
    };
    let mut apps = Vec::new();
    for app_id in ids {
        let status = app_status_value(harness, &app_id, "origin/main")?;
        let root = status
            .pointer("/repositories/0/root")
            .and_then(Value::as_str);
        apps.push(json!({
            "appId": app_id,
            "journeys": status.get("journeys").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
            "untested": status.get("untested").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
            "affectedJourneys": value_or(status.get("affectedJourneys"), json!([])),
            "eligible": value_or(status.pointer("/mergeEligibility/eligible"), Value::Bool(false)),
            "blockingReasons": value_or(status.pointer("/mergeEligibility/blockingReasons"), json!([])),
            "violations": if include_violations {
                root.map(violations_for).unwrap_or_else(|| json!({ "error": "no repository root" }))
            } else { Value::Null },
        }));
    }
    Ok(json!({ "generatedAt": now(), "apps": apps, "fleet": fleet_health() }))
}

fn render_overview(report: &Value) -> String {
    let mut lines = vec![format!(
        "overview {}",
        report
            .get("generatedAt")
            .and_then(Value::as_str)
            .unwrap_or("")
    )];
    for app in report
        .get("apps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let violation = match app.get("violations") {
            Some(Value::Object(value)) => {
                if let Some(error) = value.get("error").and_then(Value::as_str) {
                    format!(" | violations: {error}")
                } else {
                    format!(
                        " | violations: {}",
                        value.get("violations").and_then(Value::as_u64).unwrap_or(0)
                    )
                }
            }
            _ => String::new(),
        };
        lines.push(format!(
            "  {}: journeys {} (untested {}) | eligible: {}{}",
            app.get("appId").and_then(Value::as_str).unwrap_or(""),
            app.get("journeys").and_then(Value::as_u64).unwrap_or(0),
            app.get("untested").and_then(Value::as_u64).unwrap_or(0),
            app.get("eligible")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            violation,
        ));
        for reason in app
            .get("blockingReasons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(3)
            .filter_map(Value::as_str)
        {
            lines.push(format!("    - {reason}"));
        }
    }
    let fleet = report.get("fleet").unwrap_or(&Value::Null);
    if fleet.get("available").and_then(Value::as_bool) == Some(false) {
        lines.push(format!(
            "  fleet: unknown — {}",
            fleet.get("message").and_then(Value::as_str).unwrap_or("")
        ));
        lines.push(format!(
            "         ({} / {} / retryable: {})",
            fleet
                .get("failurePoint")
                .and_then(Value::as_str)
                .unwrap_or(""),
            fleet.get("errorCode").and_then(Value::as_str).unwrap_or(""),
            fleet
                .get("retryable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ));
    } else {
        let live = fleet
            .get("live")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let stale = fleet
            .get("stale")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!(
            "  fleet: live [{}] | stale [{}]",
            if live.is_empty() { "none" } else { &live },
            if stale.is_empty() { "none" } else { &stale },
        ));
    }
    lines.join("\n")
}

pub fn overview(
    harness: &Path,
    app_ids: &[String],
    text: bool,
    include_violations: bool,
) -> Answer {
    let report = overview_value(
        harness,
        (!app_ids.is_empty()).then_some(app_ids),
        include_violations,
    )?;
    if text {
        println!("{}", render_overview(&report));
        Ok(())
    } else {
        print_json(&report)
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn failures_dir() -> PathBuf {
    std::env::var_os("PROBIERZ_FAILURES_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".probierz").join("failures"))
}

fn service_file_name(service: &str) -> String {
    let mut replaced = String::new();
    let mut invalid_run = false;
    for byte in service.trim().to_ascii_lowercase().bytes() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' {
            invalid_run = false;
            replaced.push(byte as char);
        } else if !invalid_run {
            replaced.push('-');
            invalid_run = true;
        }
    }
    let clean = replaced.trim_matches('-');
    format!("{}.jsonl", if clean.is_empty() { "unknown" } else { clean })
}

fn log_failure_index(detail: &str) {
    eprintln!(
        "probierz-failure {}",
        json!({
            "failure_point": "cli.failures",
            "error_code": "unknown",
            "service": "cli",
            "impact": "cli",
            "severity": "error",
            "retryable": false,
            "outage": false,
            "detail": detail,
        })
    );
}

fn failures_index(service: Option<&str>, limit: usize) -> Value {
    let directory = failures_dir();
    let mut names = Vec::new();
    if directory.exists() {
        match fs::read_dir(&directory) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let name = entry.file_name().to_string_lossy().into_owned();
                            if name.ends_with(".jsonl") {
                                names.push(name);
                            }
                        }
                        Err(error) => {
                            log_failure_index(&format!("list the failures index: {error}"))
                        }
                    }
                }
                names.sort();
            }
            Err(error) => log_failure_index(&format!("list the failures index: {error}")),
        }
    }
    if let Some(service) = service {
        let wanted = service_file_name(service);
        names.retain(|name| name == &wanted);
    }
    let mut envelopes = Vec::new();
    let mut unparsed = 0usize;
    for name in &names {
        match fs::read_to_string(directory.join(name)) {
            Ok(text) => {
                for line in text.lines().filter(|line| !line.trim().is_empty()) {
                    match serde_json::from_str::<Value>(line) {
                        Ok(envelope) => envelopes.push(envelope),
                        Err(_) => unparsed += 1,
                    }
                }
            }
            Err(error) => log_failure_index(&format!("read {name}: {error}")),
        }
    }
    // JavaScript joins the two grouping fields with an embedded NUL, then
    // splits on that same byte. Keep it explicit here so source tooling does
    // not silently hide the separator.
    let mut grouped: Vec<(String, usize)> = Vec::new();
    for envelope in &envelopes {
        let service = string(envelope.get("service")).unwrap_or("unknown");
        let code = string(envelope.get("error_code")).unwrap_or("unknown");
        let key = format!("{service}\0{code}");
        if let Some((_, count)) = grouped.iter_mut().find(|entry| entry.0 == key) {
            *count += 1;
        } else {
            grouped.push((key, 1));
        }
    }
    let mut counts = grouped
        .into_iter()
        .map(|(key, count)| {
            let (service, code) = key.split_once('\0').unwrap_or((&key, ""));
            json!({
                "service": service,
                "error_code": code,
                "count": count,
            })
        })
        .collect::<Vec<_>>();
    counts.sort_by(|left, right| {
        number(right.get("count"))
            .partial_cmp(&number(left.get("count")))
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                string(left.get("service"))
                    .unwrap_or("")
                    .cmp(string(right.get("service")).unwrap_or(""))
            })
            .then_with(|| {
                string(left.get("error_code"))
                    .unwrap_or("")
                    .cmp(string(right.get("error_code")).unwrap_or(""))
            })
    });
    let mut ordered = envelopes.iter().enumerate().collect::<Vec<_>>();
    ordered.sort_by(|(left_index, left), (right_index, right)| {
        string(left.get("received_at"))
            .unwrap_or("")
            .cmp(string(right.get("received_at")).unwrap_or(""))
            .then_with(|| left_index.cmp(right_index))
    });
    let take = if limit == 0 { ordered.len() } else { limit };
    let newest = ordered
        .into_iter()
        .rev()
        .take(take)
        .map(|(_, envelope)| envelope.clone())
        .collect::<Vec<_>>();
    json!({
        "directory": directory.to_string_lossy(),
        "services": names.iter().map(|name| Value::String(name.trim_end_matches(".jsonl").to_string())).collect::<Vec<_>>(),
        "total": envelopes.len(),
        "unparsed": unparsed,
        "counts": counts,
        "newest": newest,
    })
}

fn trim_detail(text: &str, limit: usize) -> String {
    let value = text.trim();
    value.chars().take(limit).collect()
}

fn render_failures(report: &Value) -> String {
    let mut lines = vec![
        format!(
            "failures: {} stored ({} unparsed lines) in {}",
            report.get("total").and_then(Value::as_u64).unwrap_or(0),
            report.get("unparsed").and_then(Value::as_u64).unwrap_or(0),
            report
                .get("directory")
                .and_then(Value::as_str)
                .unwrap_or(""),
        ),
        "by service and error_code:".to_string(),
    ];
    let counts = report
        .get("counts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if counts.is_empty() {
        lines.push("  (none)".to_string());
    }
    for row in counts {
        lines.push(format!(
            "  {}  {}  {}",
            row.get("service").and_then(Value::as_str).unwrap_or(""),
            row.get("error_code").and_then(Value::as_str).unwrap_or(""),
            row.get("count").and_then(Value::as_u64).unwrap_or(0),
        ));
    }
    lines.push("newest:".to_string());
    let newest = report
        .get("newest")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if newest.is_empty() {
        lines.push("  (none)".to_string());
    }
    for envelope in newest {
        let detail = string(envelope.get("detail"))
            .map(|detail| format!("  {}", trim_detail(detail, 160)))
            .unwrap_or_default();
        lines.push(format!(
            "  {}  {}  {}  {}{}",
            string(envelope.get("received_at")).unwrap_or("-"),
            envelope
                .get("service")
                .and_then(Value::as_str)
                .unwrap_or("undefined"),
            envelope
                .get("error_code")
                .and_then(Value::as_str)
                .unwrap_or("undefined"),
            envelope
                .get("failure_point")
                .and_then(Value::as_str)
                .unwrap_or("undefined"),
            detail,
        ));
    }
    lines.join("\n")
}

pub fn failures(service: Option<&str>, limit: usize, output_json: bool) -> Answer {
    let report = failures_index(service, limit);
    if output_json {
        print_json(&report)
    } else {
        println!("{}", render_failures(&report));
        Ok(())
    }
}

fn parse_bind(bind: &str) -> Result<(&str, u16), Failure> {
    let Some((host, port)) = bind.rsplit_once(':') else {
        return Err(Failure::invalid(
            "intake.bind",
            format!("--bind needs host:port, got {bind:?}"),
        ));
    };
    if host.is_empty() || port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Failure::invalid(
            "intake.bind",
            format!("--bind needs host:port, got {bind:?}"),
        ));
    }
    let port_number = port
        .parse::<u32>()
        .ok()
        .filter(|port| (1..=65535).contains(port))
        .ok_or_else(|| {
            Failure::invalid("intake.bind", format!("--bind port out of range: {port}"))
        })?;
    Ok((host, port_number as u16))
}

fn random_token() -> Result<String, Failure> {
    let mut bytes = [0u8; 24];
    rand_core::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|error| {
            Failure::unavailable(
                "intake.token",
                format!("could not generate intake token: {error}"),
            )
        })?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn intake_token() -> Result<(String, bool, PathBuf), Failure> {
    if let Ok(token) = std::env::var("PROBIERZ_INTAKE_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok((
                token,
                false,
                home_dir().join(".probierz").join("intake-token"),
            ));
        }
    }
    let file = home_dir().join(".probierz").join("intake-token");
    if let Ok(existing) = fs::read_to_string(&file) {
        let existing = existing.trim().to_string();
        if !existing.is_empty() {
            return Ok((existing, false, file));
        }
    }
    let parent = file.parent().unwrap_or(Path::new("."));
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        let mut directories = fs::DirBuilder::new();
        directories.recursive(true).mode(0o700);
        directories.create(parent)?;
        let token = random_token()?;
        let mut output = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&file)?;
        output.write_all(format!("{token}\n").as_bytes())?;
        return Ok((token, true, file));
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(parent)?;
        let token = random_token()?;
        fs::write(&file, format!("{token}\n"))?;
        Ok((token, true, file))
    }
}

fn valid_failure_point(point: &str) -> bool {
    if point.is_empty() {
        return false;
    }
    point.split('.').all(|segment| {
        let bytes = segment.as_bytes();
        if bytes.is_empty() || !bytes[0].is_ascii_lowercase() {
            return false;
        }
        let mut previous_separator = false;
        for byte in &bytes[1..] {
            if *byte == b'-' || *byte == b'_' {
                if previous_separator {
                    return false;
                }
                previous_separator = true;
            } else if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
                previous_separator = false;
            } else {
                return false;
            }
        }
        !previous_separator
    })
}

fn envelope_problem(body: &Value) -> Option<String> {
    if !body.is_object() {
        return Some("body is not a JSON object".to_string());
    }
    if !body
        .get("failure_point")
        .and_then(Value::as_str)
        .is_some_and(valid_failure_point)
    {
        return Some("failure_point must be a dotted lowercase path".to_string());
    }
    let code = body.get("error_code").and_then(Value::as_str);
    if !code.is_some_and(|code| ERROR_CODES.contains(&code)) {
        return Some(format!(
            "error_code must be one of {}",
            ERROR_CODES.join(", ")
        ));
    }
    if !body
        .get("service")
        .and_then(Value::as_str)
        .is_some_and(|service| !service.trim().is_empty())
    {
        return Some("service must be a non-empty string".to_string());
    }
    None
}

fn code_meaning(code: &str) -> (&'static str, bool, bool) {
    match code {
        "config" => ("critical", false, true),
        "auth" | "not_found" => ("warning", false, false),
        "rate_limit" => ("warning", true, false),
        "timeout" => ("error", true, true),
        "infra_down" => ("critical", true, true),
        _ => ("error", false, false),
    }
}

fn failure_envelope(code: &str, detail: &str) -> Value {
    let (severity, retryable, outage) = code_meaning(code);
    json!({
        "failure_point": "probierz.intake.request",
        "error_code": code,
        "service": "probierz-intake",
        "impact": "failure-intake",
        "severity": severity,
        "retryable": retryable,
        "outage": outage,
        "detail": trim_detail(detail, 2000),
    })
}

fn authorized(header: Option<&str>, token: &str) -> bool {
    let presented = header
        .and_then(|header| header.strip_prefix("Bearer "))
        .unwrap_or("");
    if presented.len() != token.len() {
        return false;
    }
    presented
        .bytes()
        .zip(token.bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn bounded_line(envelope: &Value) -> Result<String, Failure> {
    let mut stored = envelope.clone();
    stored
        .as_object_mut()
        .ok_or_else(|| Failure::invalid("intake.envelope", "body is not a JSON object"))?
        .insert("received_at".to_string(), Value::String(now()));
    let mut line = serde_json::to_string(&stored)?;
    if line.len() <= MAX_LINE_BYTES {
        return Ok(line);
    }
    let Some(object) = stored.as_object_mut() else {
        return Err(Failure::invalid(
            "intake.envelope",
            "body is not a JSON object",
        ));
    };
    object.remove("context");
    line = serde_json::to_string(&stored)?;
    if line.len() <= MAX_LINE_BYTES {
        return Ok(line);
    }
    let Some(object) = stored.as_object_mut() else {
        return Err(Failure::invalid(
            "intake.envelope",
            "body is not a JSON object",
        ));
    };
    object.remove("cause");
    let mut detail = stored
        .get("detail")
        .map(Value::to_string)
        .unwrap_or_default();
    if let Some(value) = stored.get("detail").and_then(Value::as_str) {
        detail = value.to_string();
    }
    while !detail.is_empty() {
        let Some(object) = stored.as_object_mut() else {
            return Err(Failure::invalid(
                "intake.envelope",
                "body is not a JSON object",
            ));
        };
        object.insert("detail".to_string(), Value::String(detail.clone()));
        line = serde_json::to_string(&stored)?;
        if line.len() <= MAX_LINE_BYTES {
            return Ok(line);
        }
        detail = detail.chars().take(detail.chars().count() / 2).collect();
        detail = detail.trim().to_string();
    }
    let Some(object) = stored.as_object_mut() else {
        return Err(Failure::invalid(
            "intake.envelope",
            "body is not a JSON object",
        ));
    };
    object.insert("detail".to_string(), Value::Null);
    Ok(serde_json::to_string(&stored)?)
}

fn rotate_if_needed(file: &Path, incoming_bytes: usize) -> Result<(), Failure> {
    let Ok(metadata) = fs::metadata(file) else {
        return Ok(());
    };
    if metadata.len() as usize + incoming_bytes <= MAX_FILE_BYTES {
        return Ok(());
    }
    let content = fs::read(file)?;
    let keep_from = content.len().saturating_sub(MAX_FILE_BYTES / 2);
    let kept = content[keep_from..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|offset| content[(keep_from + offset + 1)..].to_vec())
        .unwrap_or_default();
    fs::write(file, kept)?;
    Ok(())
}

fn store_envelope(envelope: &Value) -> Result<(), Failure> {
    let directory = failures_dir();
    fs::create_dir_all(&directory)?;
    let line = bounded_line(envelope)?;
    let service = envelope
        .get("service")
        .and_then(Value::as_str)
        .unwrap_or("");
    let file = directory.join(service_file_name(service));
    rotate_if_needed(&file, line.len() + 1)?;
    let mut output = OpenOptions::new().create(true).append(true).open(file)?;
    output.write_all(line.as_bytes())?;
    output.write_all(b"\n")?;
    Ok(())
}

fn response(stream: &mut TcpStream, status: u16, payload: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    let reason = match status {
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
        text.len(),
    )?;
    stream.flush()
}

fn failure_response(
    stream: &mut TcpStream,
    status: u16,
    code: &str,
    detail: &str,
) -> std::io::Result<()> {
    response(stream, status, &failure_envelope(code, detail))
}

fn handle_connection(mut stream: TcpStream, token: &str) -> Result<(), Failure> {
    let mut received = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Ok(());
        }
        received.extend_from_slice(&chunk[..count]);
        if let Some(index) = received.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        if received.len() > MAX_LINE_BYTES + 16 * 1024 {
            failure_response(
                &mut stream,
                400,
                "unknown",
                &format!("body exceeds the {MAX_LINE_BYTES}-byte line cap"),
            )?;
            return Ok(());
        }
    };
    let headers = String::from_utf8_lossy(&received[..header_end]);
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let route = parts.next().unwrap_or("").split('?').next().unwrap_or("");
    let mut authorization = None;
    let mut content_length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("authorization") {
            authorization = Some(value.trim().to_string());
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse::<usize>().unwrap_or(0);
        }
    }
    if method != "POST" || route != "/v1/failures" {
        failure_response(
            &mut stream,
            404,
            "not_found",
            "unknown route; the intake endpoint is POST /v1/failures",
        )?;
        return Ok(());
    }
    if !authorized(authorization.as_deref(), token) {
        failure_response(&mut stream, 401, "auth", "missing or wrong bearer token")?;
        return Ok(());
    }
    if content_length > MAX_LINE_BYTES {
        failure_response(
            &mut stream,
            400,
            "unknown",
            &format!("body exceeds the {MAX_LINE_BYTES}-byte line cap"),
        )?;
        return Ok(());
    }
    while received.len().saturating_sub(header_end) < content_length {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        received.extend_from_slice(&chunk[..count]);
        if received.len().saturating_sub(header_end) > MAX_LINE_BYTES {
            failure_response(
                &mut stream,
                400,
                "unknown",
                &format!("body exceeds the {MAX_LINE_BYTES}-byte line cap"),
            )?;
            return Ok(());
        }
    }
    let body_bytes = &received[header_end..received.len().min(header_end + content_length)];
    let body: Value = match serde_json::from_slice(body_bytes) {
        Ok(body) => body,
        Err(_) => {
            failure_response(&mut stream, 400, "unknown", "body is not valid JSON")?;
            return Ok(());
        }
    };
    if let Some(problem) = envelope_problem(&body) {
        failure_response(&mut stream, 400, "unknown", &problem)?;
        return Ok(());
    }
    match store_envelope(&body) {
        Ok(()) => response(&mut stream, 202, &json!({ "accepted": true }))?,
        Err(error) => failure_response(
            &mut stream,
            500,
            "infra_down",
            &format!("intake store failed: {}", error.detail),
        )?,
    }
    Ok(())
}

pub fn intake_serve(bind: Option<&str>) -> Answer {
    let bind = bind.unwrap_or(DEFAULT_BIND);
    let (host, port) = parse_bind(bind)?;
    let (token, created, token_file) = intake_token()?;
    if created {
        eprintln!(
            "probierz intake: generated a new intake token at {} (mode 0600), shown once:\n{}\nSet PROBIERZ_INTAKE_TOKEN to this value in each desktop app.",
            token_file.display(),
            token,
        );
    }
    let address = (host, port)
        .to_socket_addrs()
        .map_err(|error| Failure::invalid("intake.bind", error.to_string()))?
        .next()
        .ok_or_else(|| {
            Failure::invalid(
                "intake.bind",
                format!("--bind needs host:port, got {bind:?}"),
            )
        })?;
    let listener = TcpListener::bind(address).map_err(|error| {
        Failure::unavailable("intake.listen", format!("could not bind {bind}: {error}"))
    })?;
    eprintln!("probierz intake: listening on http://{host}:{port}/v1/failures");
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let token = token.clone();
                thread::spawn(move || {
                    let _ = handle_connection(stream, &token);
                });
            }
            Err(error) => return Err(Failure::unavailable("intake.listen", error.to_string())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_has_javascript_star_semantics() {
        assert!(glob_matches("Sources/**/*.swift", "Sources/App/View.swift"));
        assert!(!glob_matches("Sources/*.swift", "Sources/App/View.swift"));
        assert!(glob_matches("Package.swift", "Package.swift"));
    }

    #[test]
    fn failure_point_requires_dotted_lowercase_segments() {
        assert!(valid_failure_point("desktop.login.auth-failed"));
        assert!(!valid_failure_point("Desktop.login"));
        assert!(!valid_failure_point("desktop..login"));
        assert!(!valid_failure_point("desktop.-login"));
    }

    #[test]
    fn service_filename_preserves_existing_hyphens() {
        assert_eq!(service_file_name(" A- B "), "a--b.jsonl");
        assert_eq!(service_file_name("%%%"), "unknown.jsonl");
    }
}
