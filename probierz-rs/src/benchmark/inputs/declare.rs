//! What a product's manifest declares under `benchmark:`, judged once.
//!
//! The section is read into typed records that refuse an unknown key, so a
//! misspelt field is a refusal naming it rather than a setting that silently
//! does nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

/// Every suite and contender one manifest declares.
#[derive(Debug)]
pub(crate) struct Declared {
    pub suites: BTreeMap<String, PathBuf>,
    pub contenders: BTreeMap<String, Contender>,
}

impl Declared {
    pub fn ours(&self) -> &Contender {
        self.contenders
            .values()
            .find(|contender| contender.ours)
            .expect("declared() refuses a manifest without exactly one ours")
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

/// A path in the manifest is absolute after `~/` expansion, or relative to
/// the manifest's own directory.
fn located(manifest: &Manifest, written: &str) -> PathBuf {
    let path = Path::new(written);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    manifest
        .file
        .parent()
        .expect("a manifest file sits in its app directory")
        .join(path)
}

pub(crate) fn declared(manifest: &Manifest) -> Result<Declared, Failure> {
    let section = manifest.document.get("benchmark").ok_or_else(|| {
        refuse(
            manifest,
            "declares no benchmark; add benchmark.suites and benchmark.contenders",
        )
    })?;
    let section: Section = serde_yaml::from_value(section.clone())
        .map_err(|error| refuse(manifest, format!("benchmark is malformed: {error}")))?;
    if section.suites.is_empty() {
        return Err(refuse(manifest, "benchmark.suites declares no suite"));
    }
    if section.contenders.is_empty() {
        return Err(refuse(
            manifest,
            "benchmark.contenders declares no contender",
        ));
    }
    let suites = section
        .suites
        .iter()
        .map(|(id, file)| (id.clone(), located(manifest, file)))
        .collect();
    let contenders: BTreeMap<String, Contender> = section
        .contenders
        .into_iter()
        .map(|(id, declaration)| {
            let contender = Contender {
                id: id.clone(),
                program: located(manifest, &declaration.program),
                args: declaration.args,
                env: declaration.env,
                ours: declaration.ours,
            };
            (id, contender)
        })
        .collect();
    let ours: Vec<&str> = contenders
        .values()
        .filter(|contender| contender.ours)
        .map(|contender| contender.id.as_str())
        .collect();
    if ours.len() != 1 {
        let found = if ours.is_empty() {
            "none".to_string()
        } else {
            ours.join(", ")
        };
        return Err(refuse(
            manifest,
            format!("exactly one benchmark contender must declare ours: true; found {found}"),
        ));
    }
    Ok(Declared { suites, contenders })
}
