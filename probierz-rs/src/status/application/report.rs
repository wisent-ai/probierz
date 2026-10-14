//! Operator status of one application against a base ref, and its text rendering.

use crate::gate::evidence_rank;
use crate::status::*;

/// The ref a repository is measured against when the caller names none: the
/// branch this checkout tracks, which git itself records. Nothing about a
/// remote's name or its default branch is assumed.
pub(crate) const TRACKED_UPSTREAM: &str = "@{upstream}";

pub(crate) fn app_status_value(
    harness: &Path,
    app_id: &str,
    base_ref: Option<&str>,
) -> Result<Value, Failure> {
    let base_ref = base_ref.unwrap_or(TRACKED_UPSTREAM);
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
        let head = git(root, &["rev-parse", "HEAD"]);
        let base = git(root, &["rev-parse", "--verify", base_ref]);
        // A diff git cannot produce is named on the repository row: read as
        // no files, its journeys looked unaffected.
        let mut diff_error = None;
        if let (Some(head), Some(base)) = (&head, &base) {
            let range = format!("{base}..{head}");
            match git_lines(root, &["diff", "--name-only", &range]) {
                Ok(files) => {
                    diff_files.extend(files.into_iter().map(|file| Path::new(root).join(file)))
                }
                Err(cause) => diff_error = Some(cause),
            }
        }
        repositories.push(json!({
            "root": root,
            "headSha": head,
            "baseRef": base_ref,
            "baseSha": base,
            "diffError": diff_error,
        }));
        measured.push((root, head));
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

pub(crate) fn render_app_status(status: &Value) -> String {
    let mut lines = vec![format!("app: {}", said(status.get("appId"), "app id"))];
    lines.push("  journeys:".to_string());
    for journey in status
        .get("journeys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = said(journey.get("journey"), "journey name");
        let Some(run) = journey.get("lastRun").filter(|value| !value.is_null()) else {
            lines.push(format!("    {name:<24} — no runs —  E0  untested"));
            continue;
        };
        let date = string(run.get("startedAt")).map_or_else(
            || said(None, "start time"),
            |started| {
                started
                    .split_once('T')
                    .map_or(started, |(day, _)| day)
                    .to_string()
            },
        );
        lines.push(format!(
            "    {name:<24} {date}  {}  {}  {}",
            said(run.get("runId"), "run id"),
            said(run.get("evidenceLevel"), "evidence level"),
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
    let eligibility = status.get("mergeEligibility");
    lines.push(format!(
        "  affected ({}..HEAD): {}",
        said(status.get("baseRef"), "base ref"),
        if affected.is_empty() {
            "(none)"
        } else {
            &affected
        },
    ));
    lines.push(format!(
        "  merge-eligibility({}, min {}): {}",
        said(eligibility.and_then(|value| value.get("mode")), "mode"),
        said(
            eligibility.and_then(|value| value.get("minimumEvidence")),
            "minimum evidence"
        ),
        if eligibility
            .and_then(|value| value.get("eligible"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            "ELIGIBLE"
        } else {
            "BLOCKED"
        },
    ));
    for reason in eligibility
        .and_then(|value| value.get("blockingReasons"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        lines.push(format!("    - {reason}"));
    }
    lines.join("\n")
}

/// The merge status of `app_id`, printed; an app that is not eligible is a
/// refusal carrying its blocking reasons, never a process exit, so the MCP
/// server that runs this in process answers the request and keeps serving.
pub fn status(harness: &Path, app_id: &str, base_ref: Option<&str>, text: bool) -> Answer {
    let report = app_status_value(harness, app_id, base_ref)?;
    if text {
        println!("{}", render_app_status(&report));
    } else {
        print_json(&report)?;
    }
    let eligibility = report.get("mergeEligibility");
    if eligibility
        .and_then(|value| value.get("eligible"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Ok(());
    }
    Err(Failure::new(
        "status",
        crate::failure::Code::Refused,
        format!(
            "{app_id} is not eligible to merge: {}",
            match eligibility {
                Some(value) => value.to_string(),
                None => format!("the status report of {app_id} carries no mergeEligibility"),
            }
        ),
    ))
}
