//! Reading and appending the register, and judging an envelope before it is
//! stored.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use serde_json::{json, Map, Value};

use crate::failure::Failure;

use super::{register_file, INCIDENT_SCHEMA, RESOLUTION_SCHEMA};

pub(super) fn lock(harness: &Path) -> Result<std::fs::File, Failure> {
    let directory = harness.join("test-results/.incidents");
    fs::create_dir_all(&directory)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join("register.lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    Ok(lock)
}

/// The four fields every stored envelope carries, refused by name when one is
/// missing. The intake listener requires the same three identifiers; the
/// register also requires the detail, because an incident nobody can read is
/// not a record of anything.
pub fn envelope_problem(envelope: &Value) -> Option<String> {
    let Some(object) = envelope.as_object() else {
        return Some("the envelope is not a JSON object".to_string());
    };
    for name in ["failure_point", "error_code", "service", "detail"] {
        let present = object
            .get(name)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        if !present {
            return Some(format!("{name} must be a non-empty string"));
        }
    }
    None
}

pub fn read_envelope(source: &str) -> Result<Value, Failure> {
    let text = if source == "-" {
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        buffer
    } else {
        fs::read_to_string(source).map_err(|error| {
            Failure::config(
                "incident.envelope",
                format!("read the envelope at {source}: {error}"),
            )
        })?
    };
    serde_json::from_str(&text).map_err(|error| {
        Failure::invalid(
            "incident.envelope",
            format!("{source} is not one JSON object: {error}"),
        )
    })
}

/// The envelope a caller spells out in flags. Only the required fields are
/// set: severity, retryability and outage belong to the code catalogue, and
/// this module does not own that table.
pub fn envelope_from_flags(
    service: Option<&str>,
    failure_point: Option<&str>,
    error_code: Option<&str>,
    detail: Option<&str>,
) -> Value {
    let mut object = Map::new();
    object.insert(
        "failure_point".to_string(),
        json!(failure_point.unwrap_or_default()),
    );
    object.insert(
        "error_code".to_string(),
        json!(error_code.unwrap_or_default()),
    );
    object.insert("service".to_string(), json!(service.unwrap_or_default()));
    object.insert("detail".to_string(), json!(detail.unwrap_or_default()));
    Value::Object(object)
}

pub fn append(harness: &Path, entry: &Value) -> Result<(), Failure> {
    let file = register_file(harness);
    if let Some(directory) = file.parent() {
        fs::create_dir_all(directory)?;
    }
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');
    let mut output = OpenOptions::new().create(true).append(true).open(&file)?;
    output.write_all(line.as_bytes())?;
    output.sync_data()?;
    Ok(())
}

fn entries(harness: &Path) -> Result<Vec<Value>, Failure> {
    let file = register_file(harness);
    if !file.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&file)?;
    let mut parsed = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: Value = serde_json::from_str(line).map_err(|error| {
            Failure::invalid(
                "incident.register",
                format!(
                    "{}:{} is not one JSON object: {error}",
                    file.display(),
                    index + 1
                ),
            )
        })?;
        if !matches!(
            entry.get("schema").and_then(Value::as_str),
            Some(INCIDENT_SCHEMA | RESOLUTION_SCHEMA)
        ) {
            return Err(Failure::invalid(
                "incident.register",
                format!(
                    "{}:{} carries an unsupported incident schema",
                    file.display(),
                    index + 1
                ),
            ));
        }
        parsed.push(entry);
    }
    Ok(parsed)
}

pub fn field<'a>(entry: &'a Value, name: &str) -> &'a str {
    entry.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// One row per incident, newest first, with its resolution folded on. Nothing
/// in the file is rewritten, so an incident's state is what the records say it
/// is rather than what a later edit made it.
pub fn folded(harness: &Path) -> Result<Vec<Value>, Failure> {
    let all = entries(harness)?;
    let mut rows: Vec<Value> = all
        .iter()
        .filter(|entry| field(entry, "schema") == INCIDENT_SCHEMA)
        .map(|entry| {
            let mut row = entry.clone();
            row["state"] = json!("open");
            row
        })
        .collect();
    for entry in all
        .iter()
        .filter(|entry| field(entry, "schema") == RESOLUTION_SCHEMA)
    {
        let id = field(entry, "incident_id");
        if let Some(row) = rows
            .iter_mut()
            .find(|row| field(row, "incident_id") == id && field(row, "state") == "open")
        {
            row["state"] = json!("resolved");
            row["resolution"] = entry.clone();
        }
    }
    rows.reverse();
    Ok(rows)
}
