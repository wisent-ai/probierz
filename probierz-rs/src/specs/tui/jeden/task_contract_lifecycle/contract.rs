//! What a retained task contract must say, and what the delivery
//! report that closes a turn must contain.
//!
//! A turn retains exactly one contract and exactly one report. The
//! report carries one entry per requirement, each with a status, an
//! explanation in words, and — when it claims the requirement is done
//! — evidence for it.

use super::*;

/// Version of the task contract this journey reads.
const CONTRACT_VERSION: u64 = 1;

/// Statuses a report entry may carry.
const ENTRY_STATUSES: [&str; 2] = ["done", "not_applicable"];

/// The contract carries its version and exactly the requirements.
pub(crate) fn check_contract(value: &Value) -> Result<(), String> {
    if value["version"] != CONTRACT_VERSION {
        return Err(format!(
            "task contract version must be {CONTRACT_VERSION}"
        ));
    }
    let mut ids = value["requirements"]
        .as_array()
        .ok_or("task contract requirements must be an array")?
        .iter()
        .filter_map(|x| x["id"].as_str())
        .collect::<Vec<_>>();
    ids.sort();
    let mut required = REQUIREMENTS.to_vec();
    required.sort();
    if ids != required {
        return Err(format!("task contract requirements differ: {ids:?}"));
    }
    Ok(())
}

/// The one contract a turn retained, for the task it was given.
pub(crate) fn check_retained_contract(events: &[Value], task: &str) -> Result<(), String> {
    let contracts = events
        .iter()
        .filter(|e| e["type"] == "task_contract")
        .collect::<Vec<_>>();
    if contracts.len() != 1 {
        return Err("a task must retain exactly one task_contract".into());
    }
    check_contract(&contracts[0]["data"])?;
    if contracts[0]["data"]["task"] != task {
        return Err("retained task contract names a different task".into());
    }
    Ok(())
}

/// The one delivery report a successful turn retained: complete, with
/// one sound entry per requirement.
pub(crate) fn check_delivery_report(events: &[Value]) -> Result<(), String> {
    let reports = events
        .iter()
        .filter(|e| e["type"] == "task_report")
        .collect::<Vec<_>>();
    if reports.len() != 1 {
        return Err("a successful task must retain one complete delivery report".into());
    }
    let report = &reports[0]["data"];
    if report["status"] != "complete" {
        return Err("blocked work must not pass as completed".into());
    }

    let mut report_keys = report["report"]
        .as_object()
        .ok_or("task report must contain a report object")?
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    report_keys.sort();
    let mut required_keys = REQUIREMENTS.to_vec();
    required_keys.sort();
    if report_keys != required_keys {
        return Err("task report must contain exactly the required entries".into());
    }

    for name in REQUIREMENTS {
        check_entry(&report["report"][name], name)?;
    }
    Ok(())
}

/// One requirement's entry: a known status, an explanation that says
/// something, and evidence whenever it claims the work is done.
fn check_entry(entry: &Value, name: &str) -> Result<(), String> {
    let status = entry["status"].as_str().unwrap_or_default();
    if !ENTRY_STATUSES.contains(&status) {
        return Err(format!("{name} has invalid report status"));
    }
    if !entry["explanation"]
        .as_str()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Err(format!("{name} has no explanation"));
    }
    let evidence = entry["evidence"]
        .as_array()
        .ok_or_else(|| format!("{name} evidence must be an array"))?;
    if status == "done"
        && !evidence
            .iter()
            .any(|r| r.as_str().is_some_and(|s| !s.trim().is_empty()))
    {
        return Err(format!("{name} done entry has no evidence"));
    }
    Ok(())
}

/// The answer a caller received and the final event in the transcript
/// must be the same words, and no contract violation may have been
/// rejected along the way.
pub(crate) fn check_final_answer(events: &[Value], answer: &Value) -> Result<(), String> {
    let final_text = events
        .iter()
        .rev()
        .find(|e| e["type"] == "final")
        .ok_or("final transcript event missing")?["data"]["text"]
        .as_str()
        .ok_or("final transcript text missing")?
        .trim();
    if final_text != answer["text"].as_str().unwrap_or("").trim() {
        return Err("final transcript and answer differ".into());
    }
    if events
        .iter()
        .any(|e| e["type"] == "contract_violation" && e["data"]["outcome"] == "rejected")
    {
        return Err("contract violation was rejected".into());
    }
    Ok(())
}
