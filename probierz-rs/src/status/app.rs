//! Operator status of one application against a base ref, and its text rendering.

use super::*;

pub(super) fn app_status_value(
    harness: &Path,
    app_id: &str,
    base_ref: &str,
) -> Result<Value, Failure> {
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

pub(super) fn render_app_status(status: &Value) -> String {
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
