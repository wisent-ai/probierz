//! Operator status of one application against a base ref, and its text rendering.

use crate::gate::evidence_rank;
use crate::status::*;

/// The ref a repository is measured against when the caller names none: the
/// branch this checkout tracks, which git itself records. Nothing about a
/// remote's name or its default branch is assumed.
pub(crate) const TRACKED_UPSTREAM: &str = "@{upstream}";

/// A git answer as the report records it: the value, or git's reason there
/// is none, each in its own field so a reader never takes a missing sha for
/// one that was never asked.
fn read(answer: Result<String, String>) -> (Option<String>, Option<String>) {
    match answer {
        Ok(value) => (Some(value), None),
        Err(cause) => (None, Some(cause)),
    }
}

pub(crate) fn app_status_value(
    harness: &Path,
    app_id: &str,
    base_ref: Option<&str>,
) -> Result<Value, Failure> {
    let base_ref = match base_ref {
        Some(named) => named,
        None => TRACKED_UPSTREAM,
    };
    let (loaded, document) = manifest_object(harness, app_id)?;
    let history = run_history_value(harness, app_id, None, None, usize::MAX)?;
    let gates = crate::gate::gate_config(&loaded, app_id)?;
    let mut repositories = Vec::new();
    let mut measured = Vec::new();
    let mut diff_files = Vec::new();
    for repository in document
        .get("repositories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let root = repository_root(app_id, repository)?;
        let checkout = Path::new(root);
        let head = crate::gate::revision(checkout, &["rev-parse", "HEAD"]);
        let base = crate::gate::revision(checkout, &["rev-parse", "--verify", base_ref]);
        // A diff git cannot produce is named on the repository row: read as
        // no files, its journeys looked unaffected.
        let mut diff_error = None;
        if let (Ok(head), Ok(base)) = (&head, &base) {
            let range = format!("{base}..{head}");
            match crate::gate::git_lines(checkout, &["diff", "--name-only", &range]) {
                Ok(files) => diff_files.extend(files.into_iter().map(|file| checkout.join(file))),
                Err(cause) => diff_error = Some(cause),
            }
        }
        let (head, head_error) = read(head);
        let (base, base_error) = read(base);
        repositories.push(json!({
            "root": root,
            "headSha": head,
            "headError": head_error,
            "baseRef": base_ref,
            "baseSha": base,
            "baseError": base_error,
            "diffError": diff_error,
        }));
        measured.push((root, head));
    }
    let affected = affected_journeys(harness, &diff_files)?;
    let runs = history
        .get("runs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    let mut latest: HashMap<String, Value> = HashMap::new();
    for run in runs {
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
        .into_iter()
        .flat_map(|map| map.keys().cloned())
        .collect::<Vec<_>>();
    journey_names.sort();
    let mut journeys = Vec::new();
    for journey in journey_names {
        let run = latest.get(&journey);
        let fresh = run.is_some_and(|run| {
            measured.iter().all(|(root, head)| {
                let recorded = run
                    .pointer("/source/repositories")
                    .and_then(Value::as_array)
                    .and_then(|recorded| {
                        recorded.iter().find(|item| {
                            string(item.get("name"))
                                .or_else(|| string(item.get("root")))
                                .is_some_and(|name| root.ends_with(name) || name == *root)
                        })
                    });
                head.is_some()
                    && recorded
                        .and_then(|item| item.get("gitSha"))
                        .and_then(Value::as_str)
                        == head.as_deref()
            })
        });
        let last_run = match run {
            Some(run) => json!({
                "runId": value_or(run.get("runId"), Value::Null),
                "target": value_or(run.get("target"), Value::Null),
                "status": value_or(run.get("status"), Value::Null),
                "startedAt": value_or(run.get("startedAt"), Value::Null),
                "evidenceLevel": crate::gate::run_evidence_level(run),
            }),
            None => Value::Null,
        };
        journeys.push(json!({
            "journey": journey,
            "lastRun": last_run,
            "fresh": fresh,
            "affected": affected.iter().any(|name| name == &journey),
        }));
    }
    // The minimum is the manifest's to declare: without it no journey can be
    // judged eligible, and the missing declaration is the blocking reason.
    let minimum = document
        .pointer("/pullRequestPolicy/minimumEvidence")
        .and_then(Value::as_str);
    let evaluated = journeys
        .iter()
        .filter(|journey| journey.get("affected").and_then(Value::as_bool) == Some(true))
        .collect::<Vec<_>>();
    let mut blocking = Vec::new();
    if minimum.is_none() {
        blocking.push(Value::String(format!(
            "app {app_id} declares no pullRequestPolicy.minimumEvidence"
        )));
    }
    for journey in &evaluated {
        let name = said(journey.get("journey"), "journey name");
        let Some(last) = journey.get("lastRun").filter(|value| !value.is_null()) else {
            blocking.push(Value::String(format!("{name}: no runs recorded")));
            continue;
        };
        match last.get("status").and_then(Value::as_str) {
            Some("passed") => {}
            Some(status) => blocking.push(Value::String(format!("{name}: last run is {status}"))),
            None => blocking.push(Value::String(format!("{name}: last run states no status"))),
        }
        if journey.get("fresh").and_then(Value::as_bool) != Some(true) {
            blocking.push(Value::String(format!(
                "{name}: evidence is older than HEAD"
            )));
        }
        // A run that states no level ranks below any minimum, and is named so.
        if let Some(minimum) = minimum {
            if last
                .get("evidenceLevel")
                .and_then(Value::as_str)
                .and_then(evidence_rank)
                < evidence_rank(minimum)
            {
                blocking.push(Value::String(format!(
                    "{name}: {} is below {minimum}",
                    said(last.get("evidenceLevel"), "evidence level")
                )));
            }
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
    let gate = match gates.pointer("/modes/pull-request") {
        Some(gate) => gate.clone(),
        None => Value::Null,
    };
    Ok(json!({
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
