//! An application's status report as the operator reads it, and the
//! `probierz status` answer that refuses an application not eligible to
//! merge.

use crate::status::*;

/// The day part of a run's start (`YYYY-MM-DD` of an RFC 3339 time), the
/// whole text when it carries no time part, or that the run states none.
fn day(run: &Value) -> String {
    match string(run.get("startedAt")) {
        Some(started) => match started.split_once('T') {
            Some((day, _)) => day.to_string(),
            None => started.to_string(),
        },
        None => said(None, "start time"),
    }
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
            lines.push(format!("    {name}  — no runs —  untested"));
            continue;
        };
        lines.push(format!(
            "    {name}  {}  {}  {}  {}",
            day(run),
            said(run.get("runId"), "run id"),
            said(run.get("evidenceLevel"), "evidence level"),
            if journey.get("fresh").and_then(Value::as_bool) == Some(true) {
                "fresh"
            } else {
                "stale"
            },
        ));
    }
    for repository in status
        .get("repositories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let root = said(repository.get("root"), "root");
        for (field, what) in [
            ("headError", "HEAD"),
            ("baseError", "base"),
            ("diffError", "diff"),
        ] {
            if let Some(cause) = string(repository.get(field)) {
                lines.push(format!("  {root}: {what} could not be read: {cause}"));
            }
        }
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
