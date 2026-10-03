//! A suite: versioned cases, each with the instruction every contender gets
//! and the assertions its answer is judged by.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use sha2::{Digest, Sha256};

use crate::benchmark::SUITE_SCHEMA;
use crate::failure::Failure;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Suite {
    pub schema: String,
    pub id: String,
    pub version: String,
    #[serde(default = "one")]
    pub repetitions: usize,
    pub cases: Vec<Case>,
}

fn one() -> usize {
    1
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Case {
    pub id: String,
    pub instruction: String,
    #[serde(default)]
    pub input: Json,
    pub assertions: Vec<Assertion>,
}

/// One check on a contender's output, at a JSON Pointer. Exactly one of
/// `equals`, `exists` and `includes` is set.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Assertion {
    pub pointer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals: Option<Json>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exists: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub includes: Option<String>,
}

/// A suite as read, with the hash two runs must share to be compared.
pub(crate) struct Loaded {
    pub suite: Suite,
    pub hash: String,
}

fn refuse(file: &Path, what: impl std::fmt::Display) -> Failure {
    Failure::config("benchmark.suite", format!("{}: {what}", file.display()))
}

pub(crate) fn load(file: &Path, declared_id: &str) -> Result<Loaded, Failure> {
    let bytes = std::fs::read(file).map_err(|error| {
        refuse(
            file,
            format!(
                "cannot be read ({error}); the manifest names this file for suite {declared_id}"
            ),
        )
    })?;
    let suite: Suite = serde_json::from_slice(&bytes)
        .map_err(|error| refuse(file, format!("is not a benchmark suite: {error}")))?;
    if suite.schema != SUITE_SCHEMA {
        return Err(refuse(
            file,
            format!("schema is {}, expected {SUITE_SCHEMA}", suite.schema),
        ));
    }
    if suite.id != declared_id {
        return Err(refuse(
            file,
            format!(
                "declares id {}, but the manifest names it suite {declared_id}",
                suite.id
            ),
        ));
    }
    if suite.version.trim().is_empty() {
        return Err(refuse(
            file,
            "version is empty; a suite is compared only within one version",
        ));
    }
    if suite.repetitions == 0 {
        return Err(refuse(
            file,
            "repetitions is 0; a case must run at least once",
        ));
    }
    if suite.cases.is_empty() {
        return Err(refuse(file, "declares no case"));
    }
    let mut seen = BTreeSet::new();
    for case in &suite.cases {
        if !seen.insert(case.id.as_str()) {
            return Err(refuse(file, format!("case {} is declared twice", case.id)));
        }
        if case.instruction.trim().is_empty() {
            return Err(refuse(
                file,
                format!("case {} has an empty instruction", case.id),
            ));
        }
        if case.assertions.is_empty() {
            return Err(refuse(
                file,
                format!(
                    "case {} has no assertion, so no answer could fail it",
                    case.id
                ),
            ));
        }
        for assertion in &case.assertions {
            if !(assertion.pointer.is_empty() || assertion.pointer.starts_with('/')) {
                return Err(refuse(file, format!("case {} pointer {} is not a JSON Pointer; it must be empty or start with /", case.id, assertion.pointer)));
            }
            let checks = [
                assertion.equals.is_some(),
                assertion.exists.is_some(),
                assertion.includes.is_some(),
            ];
            if checks.iter().filter(|set| **set).count() != 1 {
                return Err(refuse(
                    file,
                    format!(
                        "case {} assertion at {} must set exactly one of equals, exists, includes",
                        case.id, assertion.pointer
                    ),
                ));
            }
        }
    }
    let canonical = serde_json::to_vec(&suite)?;
    Ok(Loaded {
        hash: hex::encode(Sha256::digest(canonical)),
        suite,
    })
}
