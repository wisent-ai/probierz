//! What a run's records, reports and audits hide, decided by the product's
//! own manifest and nothing else: the names it declares under
//! `artifacts.redact`, the names its `secretRefs` resolve through the vault,
//! and the values those names hold in the run. A run of something no
//! manifest describes declares nothing, so every named value it would
//! publish is hidden.

use crate::manifest::*;
use regex::{Captures, Regex};
use serde_json::{json, Value as Json};

/// One run's redaction.
#[derive(Clone, Debug)]
pub struct Redaction {
    /// The names `artifacts.redact` declares, lower-cased; nothing when no
    /// manifest describes the run, which covers every name.
    names: Option<Vec<String>>,
    /// The names the manifest resolves through the vault (`secretRefs`).
    secret_refs: Vec<String>,
    /// The run's values for the names this redaction covers, each replaced
    /// wherever it appears.
    secrets: Vec<(String, String)>,
}

impl Redaction {
    /// The redaction `manifest` declares.
    pub fn declared(manifest: &Manifest) -> Redaction {
        Redaction::of_document(&manifest.document)
    }

    /// The redaction a manifest document declares: its `artifacts.redact`
    /// names, and the names of its `secretRefs`, which hold vault values
    /// whether or not a redact name also covers them.
    pub fn of_document(document: &Value) -> Redaction {
        let names = document
            .get("artifacts")
            .and_then(|artifacts| artifacts.get("redact"))
            .and_then(Value::as_sequence)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_ascii_lowercase)
            .filter(|name| !name.is_empty())
            .collect();
        let secret_refs = document
            .get("secretRefs")
            .and_then(Value::as_mapping)
            .into_iter()
            .flatten()
            .filter_map(|(name, _)| name.as_str().map(str::to_string))
            .collect();
        Redaction {
            names: Some(names),
            secret_refs,
            secrets: Vec::new(),
        }
    }

    /// The redaction of a run no manifest describes: every name is covered.
    pub fn undeclared() -> Redaction {
        Redaction {
            names: None,
            secret_refs: Vec::new(),
            secrets: Vec::new(),
        }
    }

    /// The redaction of `app_id`'s manifest under `harness`, or the
    /// undeclared one when there is no app or it has no manifest yet. A
    /// manifest that is there and cannot be loaded is refused with its cause
    /// (the one reading [`load_if_declared`] gives), never read as absent.
    pub fn of_app(harness: &Path, app_id: Option<&str>) -> Result<Redaction, Failure> {
        let declared = match app_id {
            Some(app) => load_if_declared(harness, app)?,
            None => None,
        };
        Ok(match declared {
            Some(manifest) => Redaction::declared(&manifest),
            None => Redaction::undeclared(),
        })
    }

    /// The redaction a run recorded in its run manifest (`redaction`), so a
    /// report written later hides what the run hid.
    pub fn recorded(run_manifest: &Json) -> Redaction {
        match run_manifest.get("redaction").and_then(Json::as_array) {
            Some(names) => Redaction {
                names: Some(
                    names
                        .iter()
                        .filter_map(Json::as_str)
                        .map(str::to_ascii_lowercase)
                        .collect(),
                ),
                secret_refs: Vec::new(),
                secrets: Vec::new(),
            },
            None => Redaction::undeclared(),
        }
    }

    /// What a run manifest records of this redaction: the declared names
    /// and the vault names together, or null when no manifest declared any.
    pub fn record(&self) -> Json {
        match &self.names {
            Some(names) => json!(names
                .iter()
                .cloned()
                .chain(
                    self.secret_refs
                        .iter()
                        .map(|name| name.to_ascii_lowercase())
                )
                .collect::<Vec<_>>()),
            None => Json::Null,
        }
    }

    /// Whether a value stored under `name` is hidden.
    pub fn covers(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        if self.secret_refs.iter().any(|secret| secret == name) {
            return true;
        }
        match &self.names {
            Some(names) => {
                let lower = name.to_ascii_lowercase();
                names
                    .iter()
                    .any(|declared| lower.contains(declared.as_str()))
            }
            None => true,
        }
    }

    /// This redaction with the run's values for every covered name, each
    /// then replaced wherever [`Redaction::text`] meets it.
    pub fn with_values(mut self, values: &BTreeMap<String, String>) -> Redaction {
        self.secrets = values
            .iter()
            .filter(|(name, value)| self.covers(name) && !value.is_empty())
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        self
    }

    /// `values` with every covered one replaced by its name.
    pub fn environment(&self, values: &BTreeMap<String, String>) -> Json {
        Json::Object(
            values
                .iter()
                .map(|(name, value)| {
                    let public = if self.covers(name) {
                        format!("[REDACTED:{name}]")
                    } else {
                        value.clone()
                    };
                    (name.clone(), Json::String(public))
                })
                .collect(),
        )
    }

    /// `value` with every member stored under a covered name replaced.
    pub fn value(&self, value: &Json, key: &str) -> Json {
        if self.covers(key) {
            return json!("[REDACTED]");
        }
        match value {
            Json::Array(items) => {
                Json::Array(items.iter().map(|item| self.value(item, "")).collect())
            }
            Json::Object(object) => Json::Object(
                object
                    .iter()
                    .map(|(name, item)| (name.clone(), self.value(item, name)))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }

    /// `text` with every run value of a covered name replaced, and the value
    /// of every `NAME=value`, `NAME: value` and `"name": "value"` whose name
    /// is covered.
    pub fn text(&self, text: &str) -> String {
        let mut safe = text.to_string();
        for (name, secret) in &self.secrets {
            safe = safe.replace(secret, &format!("[REDACTED:{name}]"));
        }
        let assignment = &*ASSIGNMENT;
        safe = assignment
            .replace_all(&safe, |found: &Captures<'_>| {
                if self.covers(&found["name"]) {
                    format!("{}{}[REDACTED]", &found["name"], &found["separator"])
                } else {
                    found["whole"].to_string()
                }
            })
            .into_owned();
        let field = &*FIELD;
        field
            .replace_all(&safe, |found: &Captures<'_>| {
                if self.covers(&found["name"]) {
                    format!(
                        "\"{}\"{}\"[REDACTED]\"",
                        &found["name"], &found["separator"]
                    )
                } else {
                    found["whole"].to_string()
                }
            })
            .into_owned()
    }
}

/// A `NAME=value` or `NAME: value` assignment, compiled once for every text
/// a run redacts.
static ASSIGNMENT: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"(?P<whole>\b(?P<name>[A-Za-z][A-Za-z0-9_.-]*)(?P<separator>\s*[=:]\s*)[^\s,;]+)")
        .expect("the assignment pattern compiles")
});

/// A `"name": "value"` JSON field, compiled once.
static FIELD: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r#"(?P<whole>"(?P<name>[^"]*)"(?P<separator>\s*:\s*)"[^"]*")"#)
        .expect("the field pattern compiles")
});
