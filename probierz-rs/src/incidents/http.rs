//! Desktop incident operations use the same validation and storage as the CLI.
use std::path::Path;

use serde::Deserialize;
use serde_json::{json, Value};

use super::commands::{list_rows, one, record_envelope, resolve_entry};
use crate::failure::Failure;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    claim: String,
    envelope: Value,
    run_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    state: String,
    limit: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Show {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resolve {
    id: String,
    note: String,
    run_id: Option<String>,
}

fn decode<'a, T: Deserialize<'a>>(body: &'a Value) -> Result<T, Failure> {
    T::deserialize(body).map_err(|error| Failure::invalid("incident.request", error.to_string()))
}

pub(crate) fn handle(root: &Path, path: &str, body: &Value) -> Result<Option<Value>, Failure> {
    let value = match path {
        "/v1/incidents/record" => {
            let record: Record = decode(body)?;
            record_envelope(
                root,
                &record.claim,
                record.envelope,
                record.run_id.as_deref(),
            )?
        }
        "/v1/incidents/list" => {
            let list: List = decode(body)?;
            json!({"incidents": list_rows(root, &list.state, list.limit)?})
        }
        "/v1/incidents/show" => {
            let show: Show = decode(body)?;
            one(root, &show.id)?
        }
        "/v1/incidents/resolve" => {
            let resolution: Resolve = decode(body)?;
            resolve_entry(
                root,
                &resolution.id,
                &resolution.note,
                resolution.run_id.as_deref(),
            )?
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
