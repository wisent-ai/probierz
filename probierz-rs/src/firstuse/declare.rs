//! What a product's manifest declares under `firstUse:`, judged once.
//!
//! ```yaml
//! firstUse:
//!   definition: ~/Documents/CodingProjects/Wisent/stado/stado-rs/src/onboarding_first_use.json
//!   install:                      # the product's public install instructions
//!   - [bash, -c, "curl -fsSL https://... | sh"]
//!   path: [.stado/bin]            # where the installed program lands, under HOME
//!   fixtures:                     # the user's own data a screen imports
//!     PATH: fixtures/registry-v2.json
//!   progress:                     # where the product records the journey
//!     file: .stado/onboarding.json
//!     fact: /evidence/registry_configuration_accepted
//! ```
//!
//! Every key is required but `fixtures` (a journey that imports nothing) and
//! `path`; an unknown key is a refusal naming it, so a misspelt field never
//! silently does nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value as Json};

use crate::failure::Failure;
use crate::manifest::Manifest;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Section {
    definition: String,
    install: Vec<Vec<String>>,
    #[serde(default)]
    path: Vec<String>,
    #[serde(default)]
    fixtures: BTreeMap<String, String>,
    progress: Progress,
}

/// Where the product keeps its own record of the journey, under the user's
/// home, and the JSON pointer at which it records the first-success fact.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Progress {
    pub file: String,
    pub fact: String,
}

/// One product's first-use journey as Probierz plays it.
#[derive(Debug)]
pub(crate) struct Declared {
    pub definition_file: PathBuf,
    pub definition: Json,
    pub install: Vec<Vec<String>>,
    pub path: Vec<String>,
    pub fixtures: BTreeMap<String, PathBuf>,
    pub progress: Progress,
}

impl Declared {
    pub fn describe(&self) -> Json {
        json!({
            "definition": self.definition_file.to_string_lossy(),
            "productId": self.definition["product_id"],
            "journeyId": self.definition["journey_id"],
            "journeyVersion": self.definition["journey_version"],
            "firstSuccessFact": self.definition["first_success_fact"],
            "install": self.install,
            "path": self.path,
            "fixtures": self.fixtures.iter().map(|(name, file)| (name.clone(), json!(file.to_string_lossy()))).collect::<serde_json::Map<_, _>>(),
            "progress": { "file": self.progress.file, "fact": self.progress.fact },
        })
    }
}

fn refuse(manifest: &Manifest, what: impl std::fmt::Display) -> Failure {
    Failure::config(
        "first-use.manifest",
        format!("{}: {what}", manifest.file.display()),
    )
}

/// A path in the manifest is absolute after `~/` expansion, or relative to
/// the manifest's own directory.
fn located(manifest: &Manifest, written: &str) -> PathBuf {
    let expanded = match written.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::from(written),
        },
        None => PathBuf::from(written),
    };
    if expanded.is_absolute() {
        return expanded;
    }
    manifest
        .file
        .parent()
        .expect("a manifest file sits in its app directory")
        .join(expanded)
}

/// A path under the user's home: relative, with no `..` that leaves it.
fn under_home(manifest: &Manifest, key: &str, written: &str) -> Result<(), Failure> {
    let path = Path::new(written);
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(refuse(manifest, format!("firstUse.{key} {written:?} must be a path under the user's home, relative and without ..")));
    }
    Ok(())
}

pub(crate) fn declared(manifest: &Manifest) -> Result<Declared, Failure> {
    let section = manifest.document.get("firstUse").ok_or_else(|| {
        refuse(manifest, "declares no firstUse: the product's first-use definition, install instructions, fixtures and progress record")
    })?;
    let section: Section = serde_yaml::from_value(section.clone())
        .map_err(|error| refuse(manifest, format!("firstUse is malformed: {error}")))?;
    if section.install.is_empty() || section.install.iter().any(Vec::is_empty) {
        return Err(refuse(
            manifest,
            "firstUse.install must list the public install instructions, each as a non-empty argv",
        ));
    }
    under_home(manifest, "progress.file", &section.progress.file)?;
    for entry in &section.path {
        under_home(manifest, "path", entry)?;
    }
    if !section.progress.fact.starts_with('/') {
        return Err(refuse(
            manifest,
            format!(
                "firstUse.progress.fact {:?} must be a JSON pointer, starting with /",
                section.progress.fact
            ),
        ));
    }
    let definition_file = located(manifest, &section.definition);
    let bytes = std::fs::read(&definition_file).map_err(|error| {
        refuse(
            manifest,
            format!(
                "firstUse.definition {} cannot be read: {error}",
                definition_file.display()
            ),
        )
    })?;
    let definition: Json = serde_json::from_slice(&bytes).map_err(|error| {
        refuse(
            manifest,
            format!(
                "firstUse.definition {} is not JSON: {error}",
                definition_file.display()
            ),
        )
    })?;
    let fixtures = section
        .fixtures
        .iter()
        .map(|(name, file)| (name.clone(), located(manifest, file)))
        .collect();
    Ok(Declared {
        definition_file,
        definition,
        install: section.install,
        path: section.path,
        fixtures,
        progress: section.progress,
    })
}
