//! What the four commands do, and what they refuse.

use std::num::NonZeroUsize;
use std::path::Path;

use serde_json::{json, Value};

use crate::failure::{Answer, Failure};

use super::store::lock;
use super::store::{append, envelope_from_flags, envelope_problem, field, folded, read_envelope};
use super::store::{shown, state, Filter};
use super::{
    actor, identity, now, register_file, INCIDENT_SCHEMA, REOPENING_SCHEMA, RESOLUTION_SCHEMA,
};

/// What one recording carries, so the call site reads as the record does.
pub struct Recorded<'a> {
    pub claim: &'a str,
    pub envelope: Option<&'a str>,
    pub service: Option<&'a str>,
    pub failure_point: Option<&'a str>,
    pub error_code: Option<&'a str>,
    pub detail: Option<&'a str>,
    pub run_id: Option<&'a str>,
}

fn print_json(value: &Value) -> Answer {
    crate::failure::print_json(value)
}

pub fn record(harness: &Path, recorded: Recorded<'_>, json_output: bool) -> Answer {
    if recorded.claim.trim().is_empty() {
        return Err(Failure::invalid(
            "incident.record",
            "--claim must say what was claimed",
        ));
    }
    let carried = match recorded.envelope {
        Some(source) => {
            let named = [
                recorded.service.is_some().then_some("--service"),
                recorded.failure_point.is_some().then_some("--failure-point"),
                recorded.error_code.is_some().then_some("--code"),
                recorded.detail.is_some().then_some("--detail"),
            ];
            let also: Vec<&str> = named.into_iter().flatten().collect();
            if !also.is_empty() {
                return Err(Failure::invalid(
                    "incident.record",
                    format!(
                        "--envelope carries the failure, so {} would be a second answer to the same field",
                        also.join(", ")
                    ),
                ));
            }
            read_envelope(source)?
        }
        None => envelope_from_flags(
            recorded.service,
            recorded.failure_point,
            recorded.error_code,
            recorded.detail,
        ),
    };
    let entry = record_envelope(harness, recorded.claim, carried, recorded.run_id)?;
    let incident_id = shown(&entry, "incident_id");
    if json_output {
        return print_json(&entry);
    }
    println!(
        "recorded {incident_id} in {}",
        register_file(harness).display()
    );
    Ok(())
}

pub(super) fn record_envelope(
    harness: &Path,
    claim: &str,
    envelope: Value,
    run_id: Option<&str>,
) -> Result<Value, Failure> {
    if claim.trim().is_empty() {
        return Err(Failure::invalid(
            "incident.record",
            "--claim must say what was claimed",
        ));
    }
    if let Some(problem) = envelope_problem(&envelope) {
        return Err(Failure::invalid(
            "incident.record",
            format!("the envelope is not usable: {problem}"),
        ));
    }
    let _lock = lock(harness)?;
    let recorded_at = now();
    let entry = json!({
        "schema": INCIDENT_SCHEMA,
        "incident_id": identity(&recorded_at, claim, &envelope),
        "recorded_at": recorded_at,
        "actor": actor()?,
        "claim": claim,
        "run_id": run_id,
        "envelope": envelope,
    });
    append(harness, &entry)?;
    Ok(entry)
}

pub fn list(harness: &Path, state: &str, limit: usize, json_output: bool) -> Answer {
    let rows = list_rows(harness, state, limit)?;
    if json_output {
        return print_json(&json!({
            "register": register_file(harness).display().to_string(),
            "state": state,
            "total": rows.len(),
            "incidents": rows,
        }));
    }
    if rows.is_empty() {
        println!(
            "no {state} incidents in {}",
            register_file(harness).display()
        );
        return Ok(());
    }
    for row in &rows {
        println!(
            "{}  {}  {}  {}",
            shown(row, "incident_id"),
            shown(row, "recorded_at"),
            shown(row, "state"),
            shown(row, "claim"),
        );
    }
    Ok(())
}

pub(super) fn list_rows(harness: &Path, state_named: &str, limit: usize) -> Result<Vec<Value>, Failure> {
    let _lock = lock(harness)?;
    let filter: Filter = serde_json::from_value(json!(state_named)).map_err(|error| {
        Failure::invalid(
            "incident.list",
            format!("--state is open, resolved or all, not {state_named}: {error}"),
        )
    })?;
    let Some(limit) = NonZeroUsize::new(limit) else {
        return Err(Failure::invalid(
            "incident.list",
            "--limit needs a positive number",
        ));
    };
    let mut kept = Vec::new();
    for row in folded(harness)? {
        if filter.keeps(state(&row)?) {
            kept.push(row);
        }
    }
    kept.truncate(limit.get());
    Ok(kept)
}

fn one_unlocked(harness: &Path, id: &str) -> Result<Value, Failure> {
    folded(harness)?
        .into_iter()
        .find(|row| field(row, "incident_id") == Some(id))
        .ok_or_else(|| {
            Failure::invalid(
                "incident.read",
                format!("no incident {id} in {}", register_file(harness).display()),
            )
        })
}

pub(super) fn one(harness: &Path, id: &str) -> Result<Value, Failure> {
    let _lock = lock(harness)?;
    one_unlocked(harness, id)
}

pub fn show(harness: &Path, id: &str, json_output: bool) -> Answer {
    let row = one(harness, id)?;
    if json_output {
        return print_json(&row);
    }
    println!("{} {}", shown(&row, "incident_id"), shown(&row, "state"));
    println!(
        "recorded {} by {}",
        shown(&row, "recorded_at"),
        shown(&row, "actor")
    );
    println!("claim    {}", shown(&row, "claim"));
    println!("detail   {}", shown(&row["envelope"], "detail"));
    if let Some(resolution) = row.get("resolution") {
        println!(
            "resolved {} by {}: {}",
            shown(resolution, "resolved_at"),
            shown(resolution, "actor"),
            shown(resolution, "note"),
        );
    }
    Ok(())
}

pub fn resolve(
    harness: &Path,
    id: &str,
    note: &str,
    run_id: Option<&str>,
    json_output: bool,
) -> Answer {
    let entry = resolve_entry(harness, id, note, run_id)?;
    if json_output {
        return print_json(&entry);
    }
    println!("resolved {id}");
    Ok(())
}

pub(super) fn resolve_entry(
    harness: &Path,
    id: &str,
    note: &str,
    run_id: Option<&str>,
) -> Result<Value, Failure> {
    if note.trim().is_empty() {
        return Err(Failure::invalid(
            "incident.resolve",
            "--note must say what closed it",
        ));
    }
    let _lock = lock(harness)?;
    let row = one_unlocked(harness, id)?;
    if let Some(resolution) = row.get("resolution") {
        return Err(Failure::invalid(
            "incident.resolve",
            format!(
                "{id} was resolved at {} by {}",
                shown(resolution, "resolved_at"),
                shown(resolution, "actor"),
            ),
        ));
    }
    let entry = json!({
        "schema": RESOLUTION_SCHEMA,
        "incident_id": id,
        "resolved_at": now(),
        "actor": actor()?,
        "note": note,
        "run_id": run_id,
    });
    append(harness, &entry)?;
    Ok(entry)
}

/// Open a resolved incident again. An incident that is open is refused by
/// name, so a repeated reopen records nothing twice.
pub fn reopen(harness: &Path, id: &str, note: &str, json_output: bool) -> Answer {
    if note.trim().is_empty() {
        return Err(Failure::invalid(
            "incident.reopen",
            "--note must say why the resolution did not hold",
        ));
    }
    let _lock = lock(harness)?;
    let row = one_unlocked(harness, id)?;
    if row.get("resolution").is_none() {
        return Err(Failure::invalid(
            "incident.reopen",
            format!("{id} is open; only a resolved incident can be reopened"),
        ));
    }
    let entry = json!({
        "schema": REOPENING_SCHEMA,
        "incident_id": id,
        "reopened_at": now(),
        "actor": actor()?,
        "note": note,
    });
    append(harness, &entry)?;
    if json_output {
        return print_json(&entry);
    }
    println!("reopened {id}");
    Ok(())
}
