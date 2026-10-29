//! What a product declares about its first use, judged once. It sits in
//! `first-use.yaml` beside the app's `probierz.yaml`, so declaring a journey
//! never rewrites the manifest:
//!
//! ```yaml
//! definition: ~/Documents/CodingProjects/Wisent/stado/stado-rs/src/onboarding_first_use.json
//! install:                      # the product's public install instructions
//! - [bash, ~/Documents/CodingProjects/Wisent/stado/install-stado.sh]
//! inputs: [STADO_API_URL, STADO_RELEASE_VERSION]  # what the user states
//! path: [.stado/bin]            # where the installed program lands, under HOME
//! fixtures:                     # the user's own data a screen imports
//!   PATH: fixtures/registry-v2.json
//! progress:                     # where the product records the journey
//!   file: .stado/onboarding.json
//!   fact: /evidence/registry_configuration_accepted
//! ```
//!
//! Every key is required but `fixtures` (a journey that imports nothing),
//! `inputs` (a product whose instructions ask the user for nothing) and
//! `path`; an unknown key is a refusal naming it, so a misspelt field never
//! silently does nothing. An input is a value the product's own instructions
//! ask the new user to state (Stado's quick start: the control origin and the
//! exact release); the run takes it from its environment by that name and
//! refuses to start, naming it, when it is not set. An install argument
//! written `~/...` names a checkout's file and is expanded before the run
//! moves HOME to its clean one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value as Json};

use crate::failure::Failure;
use crate::manifest::Manifest;

/// The file beside an app's manifest that declares its first use.
const FIRST_USE_FILE: &str = "first-use.yaml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Section {
    definition: String,
    install: Vec<Vec<String>>,
    #[serde(default)]
    path: Vec<String>,
    #[serde(default)]
    fixtures: BTreeMap<String, String>,
    #[serde(default)]
    inputs: Vec<String>,
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
    /// The variables a new user states themselves (their control origin,
    /// the exact release they install), passed from the run's environment.
    pub inputs: Vec<String>,
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
            "inputs": self.inputs,
            "progress": { "file": self.progress.file, "fact": self.progress.fact },
        })
    }
}

/// The file that declares the app's first use, beside its manifest.
fn first_use_file(manifest: &Manifest) -> PathBuf {
    manifest.beside(FIRST_USE_FILE)
}

fn refuse(manifest: &Manifest, what: impl std::fmt::Display) -> Failure {
    Failure::config(
        "first-use.manifest",
        format!("{}: {what}", first_use_file(manifest).display()),
    )
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
    let file = first_use_file(manifest);
    let text = std::fs::read_to_string(&file).map_err(|error| {
        refuse(manifest, format!("cannot be read ({error}): the app declares no first use; write the product's first-use definition, install instructions, fixtures and progress record there"))
    })?;
    let section: Section = serde_yaml::from_str(&text)
        .map_err(|error| refuse(manifest, format!("is malformed: {error}")))?;
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
    let definition_file = manifest.beside(&section.definition);
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
        .map(|(name, file)| (name.clone(), manifest.beside(file)))
        .collect();
    Ok(Declared {
        definition_file,
        definition,
        // An argument written `~/...` names a file of a checkout, like every
        // manifest path: it is expanded here, against the operator's home,
        // before the run moves HOME to the clean one.
        install: section
            .install
            .iter()
            .map(|argv| {
                argv.iter()
                    .map(|argument| if argument.starts_with("~/") {
                        manifest.beside(argument).to_string_lossy().into_owned()
                    } else {
                        argument.clone()
                    })
                    .collect()
            })
            .collect(),
        path: section.path,
        fixtures,
        inputs: section.inputs,
        progress: section.progress,
    })
}
