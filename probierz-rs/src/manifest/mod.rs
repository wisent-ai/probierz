//! Application manifests: what a product declares about its journeys.
//!
//! A manifest is the only place a journey's identity, its target coordinates,
//! its retention and its release policy are stated. Every rule below refuses a
//! declaration rather than repairing it, because a manifest that passes while
//! meaning something else is how a release decision gets made about the wrong
//! thing.
//! One module in parts. Every part opens with `use crate::manifest::*;`, so
//! the list below is the module's single import list.

pub(crate) use std::collections::{BTreeMap, BTreeSet};
pub(crate) use std::path::{Path, PathBuf};

pub(crate) use serde::Serialize;
pub(crate) use serde_yaml::Value;

pub(crate) use crate::failure::{Answer, Code, Failure};

mod load;
mod records;
mod redaction;
mod validate;

pub use load::*;
pub use records::*;
pub use redaction::Redaction;
pub use validate::*;

/// Declare `journey` in `document` when it is not declared yet, owned by the
/// manifest's own owner; `point` names the operation for a refusal. A
/// journey states what it proves and who owns it; a cutoff is no part of
/// it, because `probierz run` waits for a suite to its own end.
pub(crate) fn declare_journey(
    document: &mut Value,
    journey: &str,
    point: &'static str,
) -> Result<(), Failure> {
    let owner = match document.get("owner").and_then(Value::as_str) {
        Some(owner) => owner.to_string(),
        None => return Err(Failure::config(point, "the manifest names no owner")),
    };
    let journeys = document
        .get_mut("journeys")
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| Failure::config(point, "manifest journeys are required"))?;
    let mut declared = serde_yaml::Mapping::new();
    declared.insert(Value::from("owner"), Value::from(owner));
    journeys
        .entry(Value::from(journey))
        .or_insert(Value::Mapping(declared));
    Ok(())
}
