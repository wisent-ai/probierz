//! Application manifests: what a product declares about its journeys.
//!
//! A manifest is the only place a journey's identity, its target coordinates,
//! its retention and its release policy are stated. Every rule below refuses a
//! declaration rather than repairing it, because a manifest that passes while
//! meaning something else is how a release decision gets made about the wrong
//! thing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_yaml::Value;

use crate::failure::{Answer, Code, Failure};

/// Keys that must never carry a value in a manifest: they belong in
/// `secretRefs`, which resolves through the vault instead.
const SENSITIVE: [&str; 10] = [
    "auth",
    "cookie",
    "credential",
    "email",
    "key",
    "otp",
    "password",
    "pii",
    "secret",
    "session",
];

const PUBLICATION_ARTIFACT_KINDS: [&str; 3] = ["screenshot", "recording", "trace"];

/// The targets whose driver can record a screen. A journey that claims a
/// recording on a driver that cannot make one is refused.
const RECORDING_TARGETS: [&str; 6] = [
    "web",
    "mobile:ios",
    "mobile:android",
    "desktop:mac",
    "desktop:cua",
    "desktop:win",
];

pub fn target_supports_artifact_kind(target: &str, kind: &str) -> bool {
    if !PUBLICATION_ARTIFACT_KINDS.contains(&kind) {
        return false;
    }
    kind != "recording" || RECORDING_TARGETS.contains(&target)
}

/// One validated manifest, with the file it was read from.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub app_id: String,
    pub file: PathBuf,
    pub document: Value,
}

/// What `probierz apps` answers with, per product.
#[derive(Debug, Clone, Serialize)]
pub struct AppSummary {
    #[serde(rename = "appId")]
    pub app_id: String,
    pub owner: String,
    pub file: String,
    pub targets: Vec<String>,
    pub journeys: Vec<String>,
}

pub fn apps_root(harness_root: &Path) -> PathBuf {
    harness_root.join("apps")
}

fn sensitive(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    SENSITIVE.iter().any(|needle| lowered.contains(needle))
}

fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        let expected_dash = matches!(index, 8 | 13 | 18 | 23);
        if expected_dash {
            if *byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    matches!(bytes[14], b'1'..=b'5') && matches!(bytes[19] | 0x20, b'8' | b'9' | b'a' | b'b')
}

fn valid_id(value: &str) -> bool {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
}

fn require(condition: bool, file: &Path, what: &str) -> Answer {
    if condition {
        Ok(())
    } else {
        Err(Failure::new(
            "manifest.validate",
            Code::Config,
            format!("invalid app manifest: {} {what}", file.display()),
        ))
    }
}

fn map_of<'a>(value: &'a Value, key: &str) -> Option<&'a serde_yaml::Mapping> {
    value.get(key).and_then(Value::as_mapping)
}

fn string_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn sequence_of<'a>(value: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    value.get(key).and_then(Value::as_sequence)
}

fn key_names(mapping: &serde_yaml::Mapping) -> Vec<String> {
    mapping
        .keys()
        .filter_map(|key| key.as_str().map(str::to_string))
        .collect()
}

/// Judge one manifest document. Order follows the declaration itself:
/// identity, repositories, surfaces, journeys, then the policies that read
/// them.
pub fn validate(document: &Value, file: &Path) -> Answer {
    require(document.as_mapping().is_some(), file, "is not an object")?;
    require(
        document.get("schemaVersion").and_then(Value::as_u64) == Some(1),
        file,
        "schemaVersion must be 1",
    )?;
    let app_id = string_of(document, "appId").unwrap_or_default();
    require(!app_id.is_empty(), file, "appId is required")?;
    require(
        !string_of(document, "owner").unwrap_or_default().is_empty(),
        file,
        "owner is required",
    )?;
    let repositories = sequence_of(document, "repositories");
    require(
        repositories.map(|list| !list.is_empty()).unwrap_or(false),
        file,
        "repositories are required",
    )?;
    let surfaces = map_of(document, "surfaces");
    require(surfaces.is_some(), file, "surfaces are required")?;
    let journeys = map_of(document, "journeys");
    require(journeys.is_some(), file, "journeys are required")?;
    let surfaces = surfaces.expect("checked");
    let journeys = journeys.expect("checked");

    let first_use = journeys.get(Value::from("onboarding-first-use"));
    if first_use.is_some() {
        require(
            valid_id(string_of(document, "productId").unwrap_or_default()),
            file,
            "productId is required and must be stable for onboarding-first-use",
        )?;
        let retain = document
            .get("artifacts")
            .and_then(|artifacts| map_of(artifacts, "retain"));
        for name in ["pullRequestDays", "nightlyDays", "adhocDays"] {
            let days = retain
                .and_then(|map| map.get(Value::from(name)))
                .and_then(Value::as_f64);
            require(days.map(|value| value > 0.0).unwrap_or(false), file,
                &format!("artifacts.retain.{name} is required and must be positive for onboarding-first-use"))?;
        }
        let redact: Vec<String> = document
            .get("artifacts")
            .and_then(|artifacts| sequence_of(artifacts, "redact"))
            .map(|list| {
                list.iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        require(
            !redact.is_empty(),
            file,
            "artifacts.redact must contain redaction keys for onboarding-first-use",
        )?;
        for name in ["TOKEN", "SECRET", "PASSWORD", "KEY", "COOKIE", "AUTH"] {
            require(
                redact.iter().any(|entry| entry == name),
                file,
                &format!("artifacts.redact must include {name} for onboarding-first-use"),
            )?;
        }
    }

    for repository in repositories.expect("checked") {
        let root = string_of(repository, "root").unwrap_or_default();
        require(
            !root.is_empty() && Path::new(root).is_absolute(),
            file,
            "repository root must be absolute",
        )?;
        require(
            repository
                .get("mappings")
                .and_then(Value::as_sequence)
                .is_some(),
            file,
            "repository mappings are required",
        )?;
    }

    for (target_key, surface) in surfaces {
        let target = target_key.as_str().unwrap_or_default();
        require(
            surface.as_mapping().is_some(),
            file,
            &format!("surface {target} must be an object"),
        )?;
        require(
            !string_of(surface, "spec").unwrap_or_default().is_empty(),
            file,
            &format!("surface {target} spec is required"),
        )?;
        let surface_journeys = sequence_of(surface, "journeys");
        require(
            surface_journeys
                .map(|list| !list.is_empty())
                .unwrap_or(false),
            file,
            &format!("surface {target} journeys are required"),
        )?;
        for journey in surface_journeys.expect("checked") {
            let name = journey.as_str().unwrap_or_default();
            require(
                journeys.get(Value::from(name)).is_some(),
                file,
                &format!("surface {target} journey {name} is unknown"),
            )?;
        }
        if let Some(runner) = surface.get("runner") {
            require(
                runner.as_mapping().is_some(),
                file,
                &format!("surface {target} runner must be an object"),
            )?;
            let command = string_of(runner, "command").filter(|value| !value.is_empty());
            let capability = string_of(runner, "capability").filter(|value| !value.is_empty());
            require(
                command.is_some() ^ capability.is_some(),
                file,
                &format!("surface {target} runner must name exactly one command or capability"),
            )?;
            if let Some(capability) = capability {
                require(
                    crate::apphooks::supports(capability),
                    file,
                    &format!("surface {target} runner capability {capability} is unknown"),
                )?;
            }
            let args_are_strings = sequence_of(runner, "args")
                .map(|list| list.iter().all(Value::is_string))
                .unwrap_or(true);
            require(
                args_are_strings,
                file,
                &format!("surface {target} runner args must be strings"),
            )?;
        }
        for (index, override_entry) in sequence_of(surface, "journeyOverrides")
            .unwrap_or(&Vec::new())
            .iter()
            .enumerate()
        {
            require(
                override_entry.as_mapping().is_some(),
                file,
                &format!("surface {target} journeyOverrides.{index} must be an object"),
            )?;
            let when = map_of(override_entry, "when");
            require(
                when.map(|map| !map.is_empty()).unwrap_or(false),
                file,
                &format!("surface {target} journeyOverrides.{index}.when is required"),
            )?;
            let override_journeys = sequence_of(override_entry, "journeys");
            require(
                override_journeys
                    .map(|list| !list.is_empty())
                    .unwrap_or(false),
                file,
                &format!("surface {target} journeyOverrides.{index}.journeys are required"),
            )?;
            for (key, value) in when.expect("checked") {
                let name = key.as_str().unwrap_or_default();
                require(
                    !sensitive(name),
                    file,
                    &format!("surface {target} journey override {name} must not be sensitive"),
                )?;
                let scalar = value.is_string() || value.is_number() || value.is_bool();
                require(
                    scalar,
                    file,
                    &format!("surface {target} journey override {name} must be scalar"),
                )?;
            }
            for journey in override_journeys.expect("checked") {
                let name = journey.as_str().unwrap_or_default();
                require(
                    journeys.get(Value::from(name)).is_some(),
                    file,
                    &format!("surface {target} journey override {name} is unknown"),
                )?;
            }
        }
        if let Some(conditions) = map_of(surface, "conditions") {
            for name in key_names(conditions) {
                require(
                    !sensitive(&name),
                    file,
                    &format!("secret condition {name} must use secretRefs"),
                )?;
            }
        }
        if let Some(env) = map_of(surface, "env") {
            for (target_name, source_name) in env {
                require(
                    !target_name.as_str().unwrap_or_default().is_empty(),
                    file,
                    &format!("surface {target} env target is required"),
                )?;
                require(
                    !source_name.as_str().unwrap_or_default().is_empty(),
                    file,
                    &format!("surface {target} env source is required"),
                )?;
            }
        }
    }

    for (journey_key, journey) in journeys {
        let name = journey_key.as_str().unwrap_or_default();
        require(
            journey.as_mapping().is_some(),
            file,
            &format!("journey {name} must be an object"),
        )?;
        require(
            !string_of(journey, "owner").unwrap_or_default().is_empty(),
            file,
            &format!("journey {name} owner is required"),
        )?;
        let timeout = journey
            .get("timeoutMs")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        require(
            timeout > 0.0,
            file,
            &format!("journey {name} timeoutMs must be positive"),
        )?;
        let has_identity = [
            "journeyId",
            "journeyVersion",
            "journeyVersionId",
            "firstSuccessFact",
        ]
        .iter()
        .any(|field| journey.get(*field).is_some());
        if name == "onboarding-first-use" || has_identity {
            require(
                !string_of(journey, "journeyId")
                    .unwrap_or_default()
                    .is_empty(),
                file,
                &format!("journey {name} journeyId is required"),
            )?;
            require(
                !string_of(journey, "journeyVersion")
                    .unwrap_or_default()
                    .is_empty(),
                file,
                &format!("journey {name} journeyVersion is required"),
            )?;
            require(
                is_uuid(string_of(journey, "journeyVersionId").unwrap_or_default()),
                file,
                &format!("journey {name} journeyVersionId must be a UUID"),
            )?;
            require(
                !string_of(journey, "firstSuccessFact")
                    .unwrap_or_default()
                    .is_empty(),
                file,
                &format!("journey {name} firstSuccessFact is required"),
            )?;
        }
        if name == "onboarding-first-use" {
            require(
                map_of(journey, "publication").is_some(),
                file,
                &format!("journey {name} publication is required"),
            )?;
        }
        if let Some(publication) = journey.get("publication") {
            require(
                has_identity || name == "onboarding-first-use",
                file,
                &format!("journey {name} publication requires immutable journey identity"),
            )?;
            require(
                !string_of(document, "productId")
                    .unwrap_or_default()
                    .is_empty(),
                file,
                &format!("journey {name} publication requires productId"),
            )?;
            require(
                !string_of(publication, "screenId")
                    .unwrap_or_default()
                    .is_empty(),
                file,
                &format!("journey {name} publication.screenId is required"),
            )?;
            let kinds: Vec<String> = sequence_of(publication, "artifactKinds")
                .map(|list| {
                    list.iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            require(
                !kinds.is_empty(),
                file,
                &format!("journey {name} publication.artifactKinds are required"),
            )?;
            let unique: BTreeSet<&String> = kinds.iter().collect();
            require(
                unique.len() == kinds.len(),
                file,
                &format!("journey {name} publication.artifactKinds must be unique"),
            )?;
            for kind in &kinds {
                require(
                    PUBLICATION_ARTIFACT_KINDS.contains(&kind.as_str()),
                    file,
                    &format!("journey {name} publication artifact kind {kind} is unsupported"),
                )?;
            }
            let evidence = string_of(publication, "minimumEvidence").unwrap_or_default();
            require(
                matches!(evidence, "E2" | "E3"),
                file,
                &format!("journey {name} publication.minimumEvidence must be E2 or E3"),
            )?;
            require(
                publication
                    .get("redactionRequired")
                    .and_then(Value::as_bool)
                    .is_some(),
                file,
                &format!("journey {name} publication.redactionRequired must be boolean"),
            )?;
        }
    }

    // A journey may only claim a recording when at least one driver serving it
    // can make one.
    for (journey_key, journey) in journeys {
        let name = journey_key.as_str().unwrap_or_default();
        let claims_recording = journey
            .get("publication")
            .and_then(|publication| sequence_of(publication, "artifactKinds"))
            .map(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("recording")))
            .unwrap_or(false);
        if !claims_recording {
            continue;
        }
        let serving: Vec<String> = surfaces
            .iter()
            .filter(|(_, surface)| {
                sequence_of(surface, "journeys")
                    .map(|list| list.iter().any(|entry| entry.as_str() == Some(name)))
                    .unwrap_or(false)
            })
            .filter_map(|(target, _)| target.as_str().map(str::to_string))
            .collect();
        require(
            serving
                .iter()
                .any(|target| target_supports_artifact_kind(target, "recording")),
            file,
            &format!("journey {name} claims recording but none of its drivers support recording"),
        )?;
    }

    if let Some(seo) = document.get("seo") {
        require(seo.as_mapping().is_some(), file, "seo must be an object")?;
        require(
            !string_of(seo, "policy").unwrap_or_default().is_empty(),
            file,
            "seo.policy is required",
        )?;
        require(
            !string_of(seo, "brief").unwrap_or_default().is_empty(),
            file,
            "seo.brief is required",
        )?;
        let profiles = map_of(seo, "profiles");
        require(profiles.is_some(), file, "seo.profiles are required")?;
        for (profile_key, profile) in profiles.expect("checked") {
            let profile_name = profile_key.as_str().unwrap_or_default();
            require(
                matches!(
                    profile_name,
                    "pull-request" | "release" | "nightly" | "production"
                ),
                file,
                &format!("seo profile {profile_name} is unsupported"),
            )?;
            require(
                profile.as_mapping().is_some(),
                file,
                &format!("seo.profiles.{profile_name} must be an object"),
            )?;
            require(
                profile
                    .get("requireSignature")
                    .and_then(Value::as_bool)
                    .is_some(),
                file,
                &format!("seo.profiles.{profile_name}.requireSignature must be boolean"),
            )?;
            require(
                profile
                    .get("requireProductionEvidence")
                    .and_then(Value::as_bool)
                    .is_some(),
                file,
                &format!("seo.profiles.{profile_name}.requireProductionEvidence must be boolean"),
            )?;
        }
    }

    if let Some(refs) = map_of(document, "secretRefs") {
        for (key, reference) in refs {
            let name = key.as_str().unwrap_or_default();
            require(
                reference
                    .as_str()
                    .map(|value| value.starts_with("vault://"))
                    .unwrap_or(false),
                file,
                &format!("secretRefs.{name} must be a vault:// reference"),
            )?;
        }
    }

    for hook_name in ["seed", "cleanup"] {
        let hook = document.get("data").and_then(|data| data.get(hook_name));
        let Some(hook) = hook else { continue };
        let command = string_of(hook, "command").filter(|value| !value.is_empty());
        let capability = string_of(hook, "capability").filter(|value| !value.is_empty());
        require(
            command.is_some() ^ capability.is_some(),
            file,
            &format!("data.{hook_name} must name exactly one command or capability"),
        )?;
        if let Some(capability) = capability {
            require(
                crate::apphooks::supports(capability),
                file,
                &format!("data.{hook_name} capability {capability} is unknown"),
            )?;
        }
        let args_are_strings = sequence_of(hook, "args")
            .map(|list| list.iter().all(Value::is_string))
            .unwrap_or(true);
        require(
            args_are_strings,
            file,
            &format!("data.{hook_name}.args must be strings"),
        )?;
    }

    if let Some(retain) = document
        .get("artifacts")
        .and_then(|artifacts| map_of(artifacts, "retain"))
    {
        for (name, days) in retain {
            let name = name.as_str().unwrap_or_default();
            require(
                days.as_f64().map(|value| value > 0.0).unwrap_or(false),
                file,
                &format!("artifacts.retain.{name} must be positive"),
            )?;
        }
    }

    if let Some(matrix) = map_of(document, "matrix") {
        for (profile_key, profile) in matrix {
            let profile_name = profile_key.as_str().unwrap_or_default();
            require(
                profile.as_mapping().is_some(),
                file,
                &format!("matrix.{profile_name} must be an object"),
            )?;
            let targets = sequence_of(profile, "targets");
            require(
                targets.map(|list| !list.is_empty()).unwrap_or(false),
                file,
                &format!("matrix.{profile_name}.targets are required"),
            )?;
            for target in targets.expect("checked") {
                let target = target.as_str().unwrap_or_default();
                require(
                    surfaces.get(Value::from(target)).is_some(),
                    file,
                    &format!("matrix.{profile_name} target {target} has no surface"),
                )?;
            }
            if let Some(dimensions) = map_of(profile, "dimensions") {
                for (name, values) in dimensions {
                    let name = name.as_str().unwrap_or_default();
                    require(
                        !sensitive(name),
                        file,
                        &format!(
                            "matrix.{profile_name} secret dimension {name} must use secretRefs"
                        ),
                    )?;
                    let list = values.as_sequence();
                    require(
                        list.map(|items| !items.is_empty()).unwrap_or(false),
                        file,
                        &format!("matrix.{profile_name}.{name} needs values"),
                    )?;
                    let scalar = list
                        .expect("checked")
                        .iter()
                        .all(|value| value.is_string() || value.is_number() || value.is_bool());
                    require(
                        scalar,
                        file,
                        &format!("matrix.{profile_name}.{name} values must be scalar"),
                    )?;
                }
            }
            let evidence = string_of(profile, "minimumCellEvidence").unwrap_or("E3");
            require(
                matches!(evidence, "E2" | "E3"),
                file,
                &format!("matrix.{profile_name}.minimumCellEvidence must be E2 or E3"),
            )?;
            let encryption = string_of(profile, "artifactEncryption").unwrap_or("optional");
            require(
                matches!(encryption, "optional" | "required"),
                file,
                &format!("matrix.{profile_name}.artifactEncryption must be optional or required"),
            )?;
            let remove_plaintext = profile.get("removePlaintextAfterProtection");
            require(
                remove_plaintext.is_none() || remove_plaintext.and_then(Value::as_bool).is_some(),
                file,
                &format!("matrix.{profile_name}.removePlaintextAfterProtection must be boolean"),
            )?;
            let max_cells = profile
                .get("maxCells")
                .and_then(Value::as_f64)
                .unwrap_or(128.0);
            require(
                max_cells > 0.0,
                file,
                &format!("matrix.{profile_name}.maxCells must be positive"),
            )?;
            let parallel = profile
                .get("maximumParallel")
                .and_then(Value::as_f64)
                .unwrap_or(4.0);
            require(
                parallel > 0.0,
                file,
                &format!("matrix.{profile_name}.maximumParallel must be positive"),
            )?;
        }
    }

    for policy_name in ["pullRequestPolicy", "releasePolicy"] {
        let Some(policy) = document.get(policy_name) else {
            continue;
        };
        let evidence = string_of(policy, "minimumEvidence").unwrap_or("E3");
        require(
            matches!(evidence, "E2" | "E3"),
            file,
            &format!("{policy_name}.minimumEvidence must be E2 or E3"),
        )?;
        for flag in ["requireProtectedArtifacts", "requireSecretScan"] {
            let value = policy.get(flag);
            require(
                value.is_none() || value.and_then(Value::as_bool).is_some(),
                file,
                &format!("{policy_name}.{flag} must be boolean"),
            )?;
        }
        for target in sequence_of(policy, "requiredTargets").unwrap_or(&Vec::new()) {
            let target = target.as_str().unwrap_or_default();
            require(
                surfaces.get(Value::from(target)).is_some(),
                file,
                &format!("{policy_name} target {target} has no surface"),
            )?;
        }
        for journey in sequence_of(policy, "requiredJourneys").unwrap_or(&Vec::new()) {
            let name = journey.as_str().unwrap_or_default();
            require(
                journeys.get(Value::from(name)).is_some(),
                file,
                &format!("{policy_name} journey {name} is unknown"),
            )?;
        }
        if let Some(profile) = string_of(policy, "requiredMatrixProfile") {
            let known = map_of(document, "matrix")
                .and_then(|matrix| matrix.get(Value::from(profile)))
                .is_some();
            require(
                known,
                file,
                &format!("{policy_name} matrix {profile} is unknown"),
            )?;
        }
    }

    Ok(())
}

/// Read and judge one product's manifest.
pub fn load(harness_root: &Path, app_id: &str) -> Result<Manifest, Failure> {
    let clean = app_id.trim();
    if !valid_id(clean) {
        return Err(Failure::invalid(
            "manifest.load",
            format!("invalid app ID: {app_id}"),
        ));
    }
    let file = apps_root(harness_root).join(clean).join("probierz.yaml");
    if !file.exists() {
        return Err(Failure::config(
            "manifest.load",
            format!("app manifest not found: {}", file.display()),
        ));
    }
    let document: Value = serde_yaml::from_str(&std::fs::read_to_string(&file)?)?;
    validate(&document, &file)?;
    let declared = string_of(&document, "appId").unwrap_or_default();
    if declared != clean {
        return Err(Failure::config(
            "manifest.load",
            format!("app manifest ID mismatch: expected {clean}, got {declared}"),
        ));
    }
    Ok(Manifest {
        app_id: declared.to_string(),
        file,
        document,
    })
}

/// Every product that declares a manifest, in the order an operator reads.
pub fn list(harness_root: &Path) -> Result<Vec<AppSummary>, Failure> {
    let root = apps_root(harness_root);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if !entry.path().join("probierz.yaml").exists() {
            continue;
        }
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    let mut summaries = Vec::with_capacity(names.len());
    for name in names {
        let manifest = load(harness_root, &name)?;
        summaries.push(AppSummary {
            app_id: manifest.app_id.clone(),
            owner: string_of(&manifest.document, "owner")
                .unwrap_or_default()
                .to_string(),
            file: manifest.file.to_string_lossy().into_owned(),
            targets: sorted_keys(&manifest.document, "surfaces"),
            journeys: sorted_keys(&manifest.document, "journeys"),
        });
    }
    Ok(summaries)
}

fn sorted_keys(document: &Value, key: &str) -> Vec<String> {
    let mut keys: Vec<String> = map_of(document, key).map(key_names).unwrap_or_default();
    keys.sort();
    keys
}

/// The journeys a surface runs, after the first override whose conditions all
/// match. An unmatched override never contributes.
pub fn surface_journeys(surface: &Value, environment: &BTreeMap<String, String>) -> Vec<String> {
    for override_entry in sequence_of(surface, "journeyOverrides").unwrap_or(&Vec::new()) {
        let when = map_of(override_entry, "when");
        let matches = when
            .map(|map| {
                map.iter().all(|(key, value)| {
                    let name = key.as_str().unwrap_or_default();
                    let wanted = match value {
                        Value::String(text) => text.clone(),
                        Value::Number(number) => number.to_string(),
                        Value::Bool(flag) => flag.to_string(),
                        _ => return false,
                    };
                    environment.get(name).map(String::as_str).unwrap_or("") == wanted
                })
            })
            .unwrap_or(false);
        if matches {
            return sequence_of(override_entry, "journeys")
                .map(|list| {
                    list.iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
        }
    }
    sequence_of(surface, "journeys")
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}
