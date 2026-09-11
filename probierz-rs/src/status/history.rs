//! Run history: per-test and per-journey history, the performance trend, and the history answer.

use super::*;

pub(super) fn percentile(mut values: Vec<f64>, fraction: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let index = ((values.len() as f64 * fraction).ceil() as usize).saturating_sub(1);
    values.get(index.min(values.len() - 1)).copied()
}

pub(super) fn test_history(runs: &[Value]) -> Vec<Value> {
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

pub(super) fn journey_history(runs: &[Value]) -> Vec<Value> {
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

pub(super) fn performance_trend(runs: &[Value]) -> Value {
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
