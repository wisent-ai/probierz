//! What the four commands do, and what they refuse.

use std::path::Path;

use serde_json::{json, Value};

use crate::failure::{Answer, Failure};

use super::store::{append, envelope_from_flags, envelope_problem, field, folded, read_envelope};
use super::{actor, identity, now, register_file, INCIDENT_SCHEMA, RESOLUTION_SCHEMA};

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
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
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
                recorded.service.map(|_| "--service"),
                recorded.failure_point.map(|_| "--failure-point"),
                recorded.error_code.map(|_| "--code"),
                recorded.detail.map(|_| "--detail"),
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
    if let Some(problem) = envelope_problem(&carried) {
        return Err(Failure::invalid(
            "incident.record",
            format!("the envelope is not usable: {problem}"),
        ));
    }
    let recorded_at = now();
    let incident_id = identity(&recorded_at, recorded.claim, &carried);
    let entry = json!({
        "schema": INCIDENT_SCHEMA,
        "incident_id": incident_id,
        "recorded_at": recorded_at,
        "actor": actor(),
        "claim": recorded.claim,
        "run_id": recorded.run_id,
        "envelope": carried,
    });
    append(harness, &entry)?;
    if json_output {
        return print_json(&entry);
    }
    println!(
        "recorded {incident_id} in {}",
        register_file(harness).display()
    );
    Ok(())
}

pub fn list(harness: &Path, state: &str, limit: usize, json_output: bool) -> Answer {
    if !matches!(state, "open" | "resolved" | "all") {
        return Err(Failure::invalid(
            "incident.list",
            format!("--state is open, resolved or all, not {state}"),
        ));
    }
    let rows: Vec<Value> = folded(harness)?
        .into_iter()
        .filter(|row| state == "all" || field(row, "state") == state)
        .take(limit)
        .collect();
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
            field(row, "incident_id"),
            field(row, "recorded_at"),
            field(row, "state"),
            field(row, "claim"),
        );
    }
    Ok(())
}

fn one(harness: &Path, id: &str) -> Result<Value, Failure> {
    folded(harness)?
        .into_iter()
        .find(|row| field(row, "incident_id") == id)
        .ok_or_else(|| {
            Failure::invalid(
                "incident.read",
                format!("no incident {id} in {}", register_file(harness).display()),
            )
        })
}

pub fn show(harness: &Path, id: &str, json_output: bool) -> Answer {
    let row = one(harness, id)?;
    if json_output {
        return print_json(&row);
    }
    println!("{} {}", field(&row, "incident_id"), field(&row, "state"));
    println!(
        "recorded {} by {}",
        field(&row, "recorded_at"),
        field(&row, "actor")
    );
    println!("claim    {}", field(&row, "claim"));
    println!("detail   {}", field(&row["envelope"], "detail"));
    if let Some(resolution) = row.get("resolution") {
        println!(
            "resolved {} by {}: {}",
            field(resolution, "resolved_at"),
            field(resolution, "actor"),
            field(resolution, "note"),
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
    if note.trim().is_empty() {
        return Err(Failure::invalid(
            "incident.resolve",
            "--note must say what closed it",
        ));
    }
    let row = one(harness, id)?;
    if let Some(resolution) = row.get("resolution") {
        return Err(Failure::invalid(
            "incident.resolve",
            format!(
                "{id} was resolved at {} by {}",
                field(resolution, "resolved_at"),
                field(resolution, "actor"),
            ),
        ));
    }
    let entry = json!({
        "schema": RESOLUTION_SCHEMA,
        "incident_id": id,
        "resolved_at": now(),
        "actor": actor(),
        "note": note,
        "run_id": run_id,
    });
    append(harness, &entry)?;
    if json_output {
        return print_json(&entry);
    }
    println!("resolved {id}");
    Ok(())
}
