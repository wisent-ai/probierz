//! Reading and appending the register, and judging an envelope before it is
//! stored.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::num::NonZeroUsize;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::failure::Failure;

use super::{register_file, INCIDENT_SCHEMA, REOPENING_SCHEMA, RESOLUTION_SCHEMA};

/// Where an incident stands, derived from its records in the order they were
/// written: recorded open, closed by a resolution, opened by a reopening.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum State {
    Open,
    Resolved,
}

/// Which incidents `incident list` prints, as `--state` names them.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Filter {
    Open,
    Resolved,
    All,
}

impl Filter {
    pub(super) fn keeps(self, state: State) -> bool {
        match self {
            Filter::Open => state == State::Open,
            Filter::Resolved => state == State::Resolved,
            Filter::All => true,
        }
    }
}

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
/// this module does not own that table. A flag not given is null, which
/// [`envelope_problem`] refuses by name, never an empty string standing in
/// for it.
pub fn envelope_from_flags(
    service: Option<&str>,
    failure_point: Option<&str>,
    error_code: Option<&str>,
    detail: Option<&str>,
) -> Value {
    let mut object = Map::new();
    object.insert("failure_point".to_string(), json!(failure_point));
    object.insert("error_code".to_string(), json!(error_code));
    object.insert("service".to_string(), json!(service));
    object.insert("detail".to_string(), json!(detail));
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

/// One register record with the line it was read from, so a record the fold
/// cannot apply is refused naming where it is.
struct Record {
    line: usize,
    schema: String,
    incident_id: String,
    entry: Value,
}

fn entries(harness: &Path) -> Result<Vec<Record>, Failure> {
    let file = register_file(harness);
    if !file.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&file)?;
    let refused = |line: usize, why: String| {
        Failure::invalid("incident.register", format!("{}:{line} {why}", file.display()))
    };
    let mut parsed = Vec::new();
    for (line, text) in (NonZeroUsize::MIN.get()..).zip(text.lines()) {
        if text.trim().is_empty() {
            continue;
        }
        let entry: Value = serde_json::from_str(text)
            .map_err(|error| refused(line, format!("is not one JSON object: {error}")))?;
        let schema = match field(&entry, "schema") {
            Some(schema @ (INCIDENT_SCHEMA | RESOLUTION_SCHEMA | REOPENING_SCHEMA)) => schema.to_string(),
            Some(_) | None => return Err(refused(line, "carries an unsupported incident schema".into())),
        };
        let Some(incident_id) = field(&entry, "incident_id").map(str::to_string) else {
            return Err(refused(line, "names no incident_id".into()));
        };
        parsed.push(Record { line, schema, incident_id, entry });
    }
    Ok(parsed)
}

/// The text `name` of a record, when it records one.
pub fn field<'a>(entry: &'a Value, name: &str) -> Option<&'a str> {
    entry.get(name).and_then(Value::as_str)
}

/// [`field`] as a person reads it: a field the record does not hold is said.
pub fn shown<'a>(entry: &'a Value, name: &str) -> &'a str {
    match field(entry, name) {
        Some(text) => text,
        None => "(not recorded)",
    }
}

/// Where a folded row stands.
pub(super) fn state(row: &Value) -> Result<State, Failure> {
    serde_json::from_value(row["state"].clone()).map_err(|error| {
        Failure::invalid("incident.register", format!("a folded incident carries no state: {error}"))
    })
}

/// One row per incident, newest first, with its resolution folded on. Nothing
/// in the file is rewritten, so an incident's state is what the records say it
/// is rather than what a later edit made it: the records are applied in the
/// order they were written, a resolution closing the incident and a
/// reopening opening it again, and every reopening is kept on the row. A
/// resolution or reopening of an incident the register does not hold, or one
/// that does not fit the incident's state, is refused naming its line: the
/// commands that write them refuse both, so such a line is a damaged register,
/// not a record to skip.
pub fn folded(harness: &Path) -> Result<Vec<Value>, Failure> {
    let file = register_file(harness);
    let refused = |line: usize, why: String| {
        Failure::invalid("incident.register", format!("{}:{line} {why}", file.display()))
    };
    let mut rows: Vec<(String, State, Value)> = Vec::new();
    for Record { line, schema, incident_id, entry } in entries(harness)? {
        if schema == INCIDENT_SCHEMA {
            rows.push((incident_id, State::Open, entry));
            continue;
        }
        let Some((_, state, row)) = rows.iter_mut().find(|(id, _, _)| *id == incident_id) else {
            return Err(refused(line, format!("follows up incident {incident_id}, which no earlier line records")));
        };
        match (schema.as_str(), *state) {
            (RESOLUTION_SCHEMA, State::Open) => {
                *state = State::Resolved;
                row["resolution"] = entry;
            }
            (REOPENING_SCHEMA, State::Resolved) => {
                *state = State::Open;
                if let Some(object) = row.as_object_mut() {
                    object.remove("resolution");
                }
                match row.get_mut("reopenings").and_then(Value::as_array_mut) {
                    Some(reopenings) => reopenings.push(entry),
                    None => row["reopenings"] = json!([entry]),
                }
            }
            (RESOLUTION_SCHEMA, State::Resolved) => {
                return Err(refused(line, format!("resolves incident {incident_id}, which is already resolved")));
            }
            (_, State::Open) | (_, State::Resolved) => {
                return Err(refused(line, format!("reopens incident {incident_id}, which is open")));
            }
        }
    }
    let mut folded: Vec<Value> = rows
        .into_iter()
        .map(|(_, state, mut row)| {
            row["state"] = json!(state);
            row
        })
        .collect();
    folded.reverse();
    Ok(folded)
}
