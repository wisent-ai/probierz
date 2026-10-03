//! A suite: versioned cases, each with the instruction every contender gets
//! and the assertions its answer is judged by.
//!
//! A case input may hold `${NAME}` placeholders for what differs between
//! hosts, such as the origin of a fixture site. The suite's `variables` maps
//! each placeholder to the environment variable that holds its value, and
//! Probierz fills them in before a contender reads the task, so no contender
//! repeats the substitution and the suite's hash never depends on a host.

use std::collections::{BTreeMap, BTreeSet};
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
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

/// Each placeholder's value, read from the variable the suite names for it.
pub(crate) fn bound(loaded: &Loaded) -> Result<BTreeMap<String, String>, Failure> {
    let mut values = BTreeMap::new();
    for (placeholder, variable) in &loaded.suite.variables {
        let value = std::env::var(variable)
            .ok()
            .filter(|value| !value.trim().is_empty());
        let value = value.ok_or_else(|| {
            Failure::new(
                "benchmark.suite",
                crate::failure::Code::Prerequisite,
                format!(
                    "suite {} fills ${{{placeholder}}} from {variable}, which is not set in this environment",
                    loaded.suite.id
                ),
            )
        })?;
        values.insert(placeholder.clone(), value.trim().to_string());
    }
    Ok(values)
}

/// A case input with every `${NAME}` placeholder replaced by its value.
pub(crate) fn filled(value: &Json, values: &BTreeMap<String, String>) -> Json {
    match value {
        Json::String(text) => {
            Json::String(values.iter().fold(text.clone(), |text, (name, value)| {
                text.replace(&format!("${{{name}}}"), value)
            }))
        }
        Json::Array(items) => Json::Array(items.iter().map(|item| filled(item, values)).collect()),
        Json::Object(fields) => Json::Object(
            fields
                .iter()
                .map(|(key, item)| (key.clone(), filled(item, values)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// A placeholder or variable name: UPPER_SNAKE_CASE.
fn upper_snake(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
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
    for (placeholder, variable) in &suite.variables {
        if !upper_snake(placeholder) || !upper_snake(variable) {
            return Err(refuse(
                file,
                format!("variables.{placeholder} = {variable}: placeholders and variable names are UPPER_SNAKE_CASE"),
            ));
        }
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
