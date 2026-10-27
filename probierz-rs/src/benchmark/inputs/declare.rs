//! What a product's manifest declares under `benchmark:`, judged once.
//!
//! The section is read into typed records that refuse an unknown key, so a
//! misspelt field is a refusal naming it rather than a setting that silently
//! does nothing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value as Json};

use crate::failure::Failure;
use crate::manifest::Manifest;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Section {
    suites: BTreeMap<String, String>,
    contenders: BTreeMap<String, Declaration>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    ours: bool,
}

/// One program that answers benchmark tasks.
#[derive(Debug, Clone)]
pub(crate) struct Contender {
    pub id: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub ours: bool,
}

/// Every suite and contender one manifest declares, and the one contender
/// that is ours, found once when the declaration is judged.
#[derive(Debug)]
pub(crate) struct Declared {
    pub suites: BTreeMap<String, PathBuf>,
    pub contenders: BTreeMap<String, Contender>,
    ours: Contender,
}

impl Declared {
    pub fn ours(&self) -> &Contender {
        &self.ours
    }

    pub fn suite(&self, id: &str) -> Result<&PathBuf, Failure> {
        self.suites.get(id).ok_or_else(|| {
            Failure::invalid(
                "benchmark.suite",
                format!(
                    "no suite {id} is declared; declared suites: {}",
                    names(self.suites.keys())
                ),
            )
        })
    }

    /// The contenders a run names, or every declared one when it names none.
    pub fn chosen(&self, requested: &[String]) -> Result<Vec<Contender>, Failure> {
        if requested.is_empty() {
            return Ok(self.contenders.values().cloned().collect());
        }
        let mut chosen: Vec<Contender> = Vec::new();
        for id in requested {
            let contender = self.contenders.get(id).ok_or_else(|| {
                Failure::invalid(
                    "benchmark.contender",
                    format!(
                        "no contender {id} is declared; declared contenders: {}",
                        names(self.contenders.keys())
                    ),
                )
            })?;
            if chosen.iter().any(|known| known.id == *id) {
                return Err(Failure::invalid(
                    "benchmark.contender",
                    format!("contender {id} is named twice; name each contender once"),
                ));
            }
            chosen.push(contender.clone());
        }
        Ok(chosen)
    }

    pub fn describe(&self) -> Json {
        json!({
            "suites": self.suites.iter().map(|(id, path)| json!({
                "id": id, "file": path.to_string_lossy(),
            })).collect::<Vec<_>>(),
            "contenders": self.contenders.values().map(|contender| json!({
                "id": contender.id,
                "ours": contender.ours,
                "program": contender.program.to_string_lossy(),
                "args": contender.args,
                "env": contender.env,
            })).collect::<Vec<_>>(),
        })
    }
}

fn names<'a>(keys: impl Iterator<Item = &'a String>) -> String {
    let listed: Vec<&str> = keys.map(String::as_str).collect();
    if listed.is_empty() {
        "none".to_string()
    } else {
        listed.join(", ")
    }
}

fn refuse(manifest: &Manifest, what: impl std::fmt::Display) -> Failure {
    Failure::config(
        "benchmark.manifest",
        format!("{}: {what}", manifest.file.display()),
    )
}

pub(crate) fn declared(manifest: &Manifest) -> Result<Declared, Failure> {
    let section = manifest.document.get("benchmark").ok_or_else(|| {
        refuse(
            manifest,
            format!(
                "declares no benchmark; probierz benchmark author suite {} --suite <id> starts one",
                manifest.app_id
            ),
        )
    })?;
    let section: Section = serde_yaml::from_value(section.clone())
        .map_err(|error| refuse(manifest, format!("benchmark is malformed: {error}")))?;
    if section.suites.is_empty() {
        return Err(refuse(
            manifest,
            format!("benchmark.suites declares no suite; probierz benchmark author suite {} --suite <id> drafts one", manifest.app_id),
        ));
    }
    if section.contenders.is_empty() {
        return Err(refuse(
            manifest,
            format!("benchmark.contenders declares no contender; probierz benchmark author contender {} --contender <id> --ours --suite <id> drafts ours", manifest.app_id),
        ));
    }
    let suites = section
        .suites
        .iter()
        .map(|(id, file)| (id.clone(), manifest.beside(file)))
        .collect();
    let contenders: BTreeMap<String, Contender> = section
        .contenders
        .into_iter()
        .map(|(id, declaration)| {
            let contender = Contender {
                id: id.clone(),
                program: manifest.beside(&declaration.program),
                args: declaration.args,
                env: declaration.env,
                ours: declaration.ours,
            };
            (id, contender)
        })
        .collect();
    let ours: Vec<&Contender> = contenders.values().filter(|contender| contender.ours).collect();
    let [ours] = ours.as_slice() else {
        let found = if ours.is_empty() {
            "none".to_string()
        } else {
            ours.iter().map(|contender| contender.id.as_str()).collect::<Vec<_>>().join(", ")
        };
        return Err(refuse(
            manifest,
            format!("exactly one benchmark contender must declare ours: true; found {found}"),
        ));
    };
    let ours = (*ours).clone();
    Ok(Declared { suites, contenders, ours })
}
