use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::failure::{fail, now_iso, print_json, write_private, Answer, Failure};

const INDEX_SCHEMA: &str = "ai.wisent.probierz.project-adoptions.v1";
const RESULT_SCHEMA: &str = "ai.wisent.probierz.project-adoption-result.v1";
const INDEX_RELATIVE_PATH: &str = "apps/.adoptions.json";
const SPEC_DIRECTORIES: [&str; 3] = ["test/specs", "tests", "specs"];
const TARGET_PACKAGES: [(&str, &str); 9] = [
    ("web", "packages/web"),
    ("electron", "packages/electron"),
    ("mobile:ios", "packages/mobile"),
    ("mobile:ios:byk-auth", "packages/mobile"),
    ("mobile:android", "packages/mobile"),
    ("desktop:mac", "packages/desktop-native"),
    ("desktop:win", "packages/desktop-native"),
    ("desktop:cua", "packages/desktop-cua"),
    ("tui", "packages/tui"),
];

#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// Adopt existing application manifests and journey specs without running them.
    Adopt {
        #[arg(long, value_name = "repository")]
        source: PathBuf,
        #[arg(long)]
        replace: bool,
    },
    /// List the retained identities of adopted definition sources.
    Adoptions,
}

#[derive(Clone)]
struct DefinitionFile {
    relative: String,
    source: PathBuf,
    mode: u32,
    bytes: Vec<u8>,
    sha256: String,
}

struct Definitions {
    application_ids: Vec<String>,
    files: Vec<DefinitionFile>,
    skipped_local_state: Vec<String>,
    source_digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RetainedFile {
    path: String,
    sha256: String,
    mode: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceRecord {
    source_key: String,
    source_root: String,
    #[serde(default)]
    source_digest: String,
    #[serde(default)]
    adopted_at: String,
    #[serde(default)]
    applications: Vec<String>,
    files: Vec<RetainedFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AdoptionIndex {
    schema: String,
    sources: Vec<SourceRecord>,
}

#[derive(Clone)]
struct Conflict {
    path: String,
    reason: &'static str,
    existing_sha256: Value,
    incoming_sha256: Value,
}

struct Counts {
    imported: usize,
    unchanged: usize,
    removed: usize,
    rejected: usize,
}

pub fn dispatch(project_root: &Path, command: ProjectCommand) -> Answer {
    match command {
        ProjectCommand::Adopt { source, replace } => {
            let result = adopt_project(project_root, &source, replace)?;
            let accepted = result.get("status").and_then(Value::as_str) != Some("conflict");
            print_json(&result)?;
            if !accepted {
                std::process::exit(1);
            }
            Ok(())
        }
        ProjectCommand::Adoptions => print_json(&list_project_adoptions(project_root)?),
    }
}

/// Validate and transactionally retain definitions from another Probierz checkout.
pub fn adopt_project(
    project_root: &Path,
    source_root: &Path,
    replace: bool,
) -> Result<Value, Failure> {
    let destination = repository_root(project_root, "Probierz project root")?;
    let source = repository_root(source_root, "Adoption source")?;
    if source == destination {
        return Err(fail(
            "adoption.source",
            "adoption source is already this Probierz project",
        ));
    }

    let definitions = source_definitions(&source)?;
    let mut index = read_index(&destination)?;
    let source_root_text = path_text(&source);
    let source_key = sha256(source_root_text.as_bytes());
    let existing_source = index
        .sources
        .iter()
        .find(|entry| entry.source_key == source_key);
    let previous_by_path: HashMap<&str, &RetainedFile> = existing_source
        .map(|entry| {
            entry
                .files
                .iter()
                .map(|file| (file.path.as_str(), file))
                .collect()
        })
        .unwrap_or_default();
    let ownership = file_owners(&index);
    let incoming: BTreeSet<&str> = definitions
        .files
        .iter()
        .map(|file| file.relative.as_str())
        .collect();
    let mut conflicts = Vec::new();
    let mut planned = Vec::new();
    let mut unchanged = 0usize;

    for file in &definitions.files {
        let target = absolute(&destination, &file.relative)?;
        let current = current_file(&target)?;
        if let CurrentFile::Regular { sha256, mode } = &current {
            if sha256 == &file.sha256 && *mode == file.mode {
                unchanged += 1;
                continue;
            }
        }

        let other_owner = ownership
            .get(file.relative.as_str())
            .is_some_and(|owners| owners.iter().any(|owner| owner != &source_key));
        let previous = previous_by_path.get(file.relative.as_str()).copied();
        let locally_changed = match (&current, previous) {
            (CurrentFile::Regular { sha256, mode }, Some(previous)) => {
                sha256 != &previous.sha256 || *mode != previous.mode
            }
            _ => false,
        };
        if other_owner {
            conflicts.push(conflict(
                &file.relative,
                "destination is owned by another adopted source",
                current.digest_value(),
                Value::String(file.sha256.clone()),
            ));
        } else if matches!(current, CurrentFile::Unsupported) {
            conflicts.push(conflict(
                &file.relative,
                "destination is not a regular file",
                Value::String("unsupported".to_string()),
                Value::String(file.sha256.clone()),
            ));
        } else if locally_changed {
            conflicts.push(conflict(
                &file.relative,
                "previously adopted definition has local content or mode changes",
                current.digest_value(),
                Value::String(file.sha256.clone()),
            ));
        } else if !matches!(current, CurrentFile::Missing) && !replace {
            conflicts.push(conflict(
                &file.relative,
                "destination content or mode differs; repeat with explicit replacement",
                current.digest_value(),
                Value::String(file.sha256.clone()),
            ));
        } else {
            planned.push(file);
        }
    }

    let mut removals = Vec::new();
    if let Some(existing) = existing_source {
        for previous in &existing.files {
            if incoming.contains(previous.path.as_str()) {
                continue;
            }
            let target = absolute(&destination, &previous.path)?;
            let current = current_file(&target)?;
            if matches!(current, CurrentFile::Missing) {
                continue;
            }
            if !replace {
                conflicts.push(conflict(
                    &previous.path,
                    "previously adopted definition is absent from the selected source",
                    current.digest_value(),
                    Value::Null,
                ));
            } else if !matches!(
                &current,
                CurrentFile::Regular { sha256, mode }
                    if sha256 == &previous.sha256 && *mode == previous.mode
            ) {
                conflicts.push(conflict(
                    &previous.path,
                    "previously adopted definition has local content or mode changes",
                    current.digest_value(),
                    Value::Null,
                ));
            } else {
                removals.push(previous.path.clone());
            }
        }
    }

    if !conflicts.is_empty() {
        let rejected = conflicts.len();
        return Ok(result(
            "conflict",
            &source_root_text,
            &definitions,
            Counts {
                imported: 0,
                unchanged,
                removed: 0,
                rejected,
            },
            &conflicts,
        ));
    }

    if existing_source.is_some_and(|entry| entry.source_digest == definitions.source_digest)
        && planned.is_empty()
        && removals.is_empty()
    {
        return Ok(result(
            "unchanged",
            &source_root_text,
            &definitions,
            Counts {
                imported: 0,
                unchanged,
                removed: 0,
                rejected: 0,
            },
            &[],
        ));
    }

    let had_existing_source = existing_source.is_some();
    let record = SourceRecord {
        source_key: source_key.clone(),
        source_root: source_root_text.clone(),
        source_digest: definitions.source_digest.clone(),
        adopted_at: now_iso(),
        applications: definitions.application_ids.clone(),
        files: definitions
            .files
            .iter()
            .map(|file| RetainedFile {
                path: file.relative.clone(),
                sha256: file.sha256.clone(),
                mode: file.mode,
            })
            .collect(),
    };
    index.sources.retain(|entry| entry.source_key != source_key);
    index.sources.push(record);
    index
        .sources
        .sort_by(|left, right| left.source_root.cmp(&right.source_root));

    apply_transaction(&destination, &planned, &removals, &index)?;
    Ok(result(
        if had_existing_source {
            "replaced"
        } else {
            "imported"
        },
        &source_root_text,
        &definitions,
        Counts {
            imported: planned.len(),
            unchanged,
            removed: removals.len(),
            rejected: 0,
        },
        &[],
    ))
}

/// Read retained source identities without exposing adopted definition contents.
pub fn list_project_adoptions(project_root: &Path) -> Result<Value, Failure> {
    let root = repository_root(project_root, "Probierz project root")?;
    let index = read_index(&root)?;
    let sources: Vec<Value> = index
        .sources
        .into_iter()
        .map(|source| {
            json!({
                "sourceKey": source.source_key,
                "sourceRoot": source.source_root,
                "sourceDigest": source.source_digest,
                "adoptedAt": source.adopted_at,
                "applications": source.applications,
                "fileCount": source.files.len(),
            })
        })
        .collect();
    Ok(json!({
        "schema": INDEX_SCHEMA,
        "file": path_text(&root.join(INDEX_RELATIVE_PATH)),
        "sources": sources,
    }))
}

/// Render the first-use journey and optionally adopt definitions before it.
pub fn onboarding(
    project_root: &Path,
    reset_requested: bool,
    source_root: Option<&Path>,
    replace: bool,
    json_output: bool,
) -> Result<bool, Failure> {
    if replace && source_root.is_none() {
        return Err(fail(
            "onboarding.options",
            "--replace requires --source <repository>",
        ));
    }
    let definition = onboarding_definition();
    let adoption = source_root
        .map(|source| adopt_project(project_root, source, replace))
        .transpose()?;

    let mut progress = read_progress();
    let mut reset = false;
    if reset_requested && progress.is_some() {
        progress = None;
        reset = true;
        write_initial_progress(&definition)?;
    } else if progress.is_none() {
        write_initial_progress(&definition)?;
    }

    if adoption
        .as_ref()
        .is_some_and(|value| value.get("status").and_then(Value::as_str) != Some("conflict"))
    {
        let adopted = adoption.as_ref().expect("checked");
        let mut adopted_progress = read_progress().unwrap_or_else(|| json!({}));
        let object = adopted_progress
            .as_object_mut()
            .ok_or_else(|| fail("onboarding.state", "onboarding progress is not an object"))?;
        object.insert("product_id".to_string(), definition["product_id"].clone());
        object.insert("journey_id".to_string(), definition["journey_id"].clone());
        object.insert(
            "journey_version".to_string(),
            definition["journey_version"].clone(),
        );
        object
            .entry("status".to_string())
            .or_insert_with(|| Value::String("in_progress".to_string()));
        let evidence = object
            .entry("evidence".to_string())
            .or_insert_with(|| json!({}));
        let evidence = evidence
            .as_object_mut()
            .ok_or_else(|| fail("onboarding.state", "onboarding evidence is not an object"))?;
        evidence.insert("project_definitions_adopted".to_string(), Value::Bool(true));
        object.insert(
            "adoption".to_string(),
            json!({
                "source_root": adopted["sourceRoot"],
                "source_digest": adopted["sourceDigest"],
                "accepted_at": now_iso(),
            }),
        );
        write_progress(&adopted_progress)?;
        progress = read_progress();
    }

    let done = progress
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        == Some("completed");
    let screens = ordered_screens(&definition);

    if json_output {
        let rendered: Vec<Value> = screens
            .iter()
            .map(|screen| {
                json!({
                    "screen_id": screen["screen_id"],
                    "title": screen.pointer("/presentation/title").cloned().unwrap_or_else(|| screen["title_key"].clone()),
                    "body": screen.pointer("/presentation/body").cloned().unwrap_or_else(|| screen["body_key"].clone()),
                    "command": screen.pointer("/presentation/command").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        return print_json(&json!({
            "product_id": definition["product_id"],
            "journey_id": definition["journey_id"],
            "journey_version": definition["journey_version"],
            "source_revision": definition["source_revision"],
            "first_success_fact": definition["first_success_fact"],
            "status": if done { "completed" } else { "in_progress" },
            "reset": reset,
            "adoption": adoption,
            "screens": rendered,
        }))
        .map(|_| true);
    }

    if reset {
        println!("First-run walkthrough reset: walkthrough progress and its first-success evidence discarded, showing it again now.");
        println!();
    }
    let mut accepted = true;
    if let Some(adoption) = adoption.as_ref() {
        if adoption.get("status").and_then(Value::as_str) == Some("conflict") {
            accepted = false;
            println!(
                "Existing project not adopted: {} conflicting definition(s). No files changed.",
                adoption["conflicting"]
            );
            if let Some(conflicts) = adoption.get("conflicts").and_then(Value::as_array) {
                for item in conflicts {
                    println!(
                        "       {}: {}",
                        item["path"].as_str().unwrap_or_default(),
                        item["reason"].as_str().unwrap_or_default()
                    );
                }
            }
            println!(
                "       Resolve the files or repeat with --replace after reviewing the conflicts."
            );
        } else {
            println!(
                "Existing project {}: {} imported, {} unchanged, {} removed.",
                adoption["status"].as_str().unwrap_or_default(),
                adoption["imported"],
                adoption["unchanged"],
                adoption["removed"]
            );
            println!("       Journey definitions were persisted but not run.");
        }
        println!();
    }
    for (index, screen) in screens.iter().enumerate() {
        let title = screen
            .pointer("/presentation/title")
            .or_else(|| screen.get("title_key"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let body = screen
            .pointer("/presentation/body")
            .or_else(|| screen.get("body_key"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        println!("{}/{}  {title}", index + 1, screens.len());
        println!("       {body}");
        if let Some(command) = screen
            .pointer("/presentation/command")
            .and_then(Value::as_str)
        {
            println!("       $ {command}");
        }
        println!();
    }
    let fact = definition["first_success_fact"]
        .as_str()
        .unwrap_or_default();
    if done {
        println!("First-run journey already complete: {fact} was observed on an earlier run.");
    } else {
        println!("No passing quality evidence written from this shell yet, so {fact} is still open; the next passing completed run closes it.");
    }
    Ok(accepted)
}

/// Progress recording must never interfere with the evidence operation that calls it.
pub fn record_passing_quality_evidence_written() {
    let _ = (|| -> Result<(), Failure> {
        let mut progress = read_progress().unwrap_or_else(|| json!({}));
        if progress.get("status").and_then(Value::as_str) == Some("completed") {
            return Ok(());
        }
        let definition = onboarding_definition();
        let object = progress
            .as_object_mut()
            .ok_or_else(|| fail("onboarding.state", "onboarding progress is not an object"))?;
        object.insert("product_id".to_string(), definition["product_id"].clone());
        object.insert("journey_id".to_string(), definition["journey_id"].clone());
        object.insert(
            "journey_version".to_string(),
            definition["journey_version"].clone(),
        );
        object.insert("status".to_string(), Value::String("completed".to_string()));
        let evidence = object
            .entry("evidence".to_string())
            .or_insert_with(|| json!({}));
        let evidence = evidence
            .as_object_mut()
            .ok_or_else(|| fail("onboarding.state", "onboarding evidence is not an object"))?;
        evidence.insert(
            definition["first_success_fact"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            Value::Bool(true),
        );
        object.insert("completed_at".to_string(), Value::String(now_iso()));
        write_progress(&progress)
    })();
}

fn source_definitions(source_root: &Path) -> Result<Definitions, Failure> {
    let apps_root = source_root.join("apps");
    let apps_metadata = fs::symlink_metadata(&apps_root).map_err(|_| {
        fail(
            "adoption.source",
            format!(
                "selected repository has no supported Probierz apps directory: {}",
                apps_root.display()
            ),
        )
    })?;
    if apps_metadata.file_type().is_symlink() || !apps_metadata.is_dir() {
        return Err(fail(
            "adoption.source",
            format!(
                "selected repository has no supported Probierz apps directory: {}",
                apps_root.display()
            ),
        ));
    }

    let mut entries: Vec<_> = fs::read_dir(&apps_root)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut application_ids = Vec::new();
    let mut manifests = Vec::new();
    for entry in entries {
        let name = entry.file_name();
        if os_text(&name).starts_with('.') {
            continue;
        }
        let app_root = entry.path();
        let metadata = fs::symlink_metadata(&app_root)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(fail(
                "adoption.source",
                format!(
                    "unsupported entry in Probierz apps directory: {}",
                    app_root.display()
                ),
            ));
        }
        let manifest_file = app_root.join("probierz.yaml");
        if !manifest_file.exists() {
            continue;
        }
        let manifest_metadata = fs::symlink_metadata(&manifest_file)?;
        if manifest_metadata.file_type().is_symlink() || !manifest_metadata.is_file() {
            return Err(fail(
                "adoption.source",
                format!(
                    "Probierz app directory has a non-regular probierz.yaml: {}",
                    app_root.display()
                ),
            ));
        }
        let body = fs::read_to_string(&manifest_file)?;
        let document: serde_yaml::Value = serde_yaml::from_str(&body)?;
        crate::manifest::validate(&document, &manifest_file)?;
        let declared = document
            .get("appId")
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or_default();
        let expected = os_text(&name);
        if declared != expected {
            return Err(fail(
                "adoption.source",
                format!("invalid app manifest: expected appId {expected}, got {declared}"),
            ));
        }
        application_ids.push(declared.to_string());
        manifests.push((document, manifest_file));
    }
    if manifests.is_empty() {
        return Err(fail(
            "adoption.source",
            format!(
                "selected repository contains no Probierz application manifests: {}",
                apps_root.display()
            ),
        ));
    }

    let mut roots = vec!["apps".to_string()];
    let mut seen = BTreeSet::from(["apps".to_string()]);
    for package in TARGET_PACKAGES.iter().map(|(_, package)| *package) {
        for directory in SPEC_DIRECTORIES {
            let relative = format!("{package}/{directory}");
            if source_root.join(&relative).exists() && seen.insert(relative.clone()) {
                roots.push(relative);
            }
        }
    }

    let mut gathered = Vec::new();
    for root in roots {
        gathered.extend(files_below(source_root, &root)?);
    }
    let skipped_local_state: Vec<String> = gathered
        .iter()
        .filter(|file| file.relative == INDEX_RELATIVE_PATH)
        .map(|file| file.relative.clone())
        .collect();
    gathered.retain(|file| file.relative != INDEX_RELATIVE_PATH);
    let relative_files: BTreeSet<&str> =
        gathered.iter().map(|file| file.relative.as_str()).collect();

    for (document, file) in manifests {
        let Some(surfaces) = document
            .get("surfaces")
            .and_then(serde_yaml::Value::as_mapping)
        else {
            continue;
        };
        for (target, surface) in surfaces {
            let target = target.as_str().unwrap_or_default();
            let package = target_package(target).ok_or_else(|| {
                fail(
                    "adoption.source",
                    format!(
                        "invalid app manifest: {} surface {target} has no supported Probierz package",
                        file.display()
                    ),
                )
            })?;
            let declared_spec = surface
                .get("spec")
                .and_then(serde_yaml::Value::as_str)
                .unwrap_or_default();
            let package_prefix = format!("{package}/");
            let found = relative_files
                .iter()
                .filter_map(|relative| relative.strip_prefix(&package_prefix))
                .any(|relative| matches_declared_spec(declared_spec, relative));
            if !found {
                return Err(fail(
                    "adoption.source",
                    format!(
                        "invalid app manifest: {} surface {target} spec {declared_spec} was not found in {package}",
                        file.display()
                    ),
                ));
            }
        }
    }

    for file in &mut gathered {
        file.bytes = fs::read(&file.source)?;
        file.sha256 = sha256(&file.bytes);
    }
    let mut identity = Sha256::new();
    for file in &gathered {
        identity.update(file.relative.as_bytes());
        identity.update([0]);
        identity.update(file.sha256.as_bytes());
        identity.update([0]);
        identity.update(file.mode.to_string().as_bytes());
        identity.update([0]);
    }
    Ok(Definitions {
        application_ids,
        files: gathered,
        skipped_local_state,
        source_digest: hex::encode(identity.finalize()),
    })
}

fn files_below(root: &Path, relative_root: &str) -> Result<Vec<DefinitionFile>, Failure> {
    let start = absolute(root, relative_root)?;
    let metadata = fs::symlink_metadata(&start)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(fail(
            "adoption.source",
            format!(
                "unsupported project definition directory: {}",
                start.display()
            ),
        ));
    }
    let mut pending = vec![relative_root.to_string()];
    let mut files = Vec::new();
    while let Some(current) = pending.pop() {
        let directory = absolute(root, &current)?;
        let mut entries: Vec<_> = fs::read_dir(directory)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = os_text(&entry.file_name()).to_string();
            let relative = format!("{current}/{name}");
            let source = absolute(root, &relative)?;
            let metadata = fs::symlink_metadata(&source)?;
            if metadata.file_type().is_symlink() {
                return Err(fail(
                    "adoption.source",
                    format!(
                        "project definitions must not contain symlinks: {}",
                        source.display()
                    ),
                ));
            }
            if metadata.is_dir() {
                pending.push(relative);
            } else if metadata.is_file() {
                files.push(DefinitionFile {
                    relative,
                    source,
                    mode: metadata_mode(&metadata),
                    bytes: Vec::new(),
                    sha256: String::new(),
                });
            } else {
                return Err(fail(
                    "adoption.source",
                    format!(
                        "project definitions contain an unsupported filesystem entry: {}",
                        source.display()
                    ),
                ));
            }
        }
    }
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}

fn read_index(project_root: &Path) -> Result<AdoptionIndex, Failure> {
    let file = absolute(project_root, INDEX_RELATIVE_PATH)?;
    if !file.exists() {
        return Ok(AdoptionIndex {
            schema: INDEX_SCHEMA.to_string(),
            sources: Vec::new(),
        });
    }
    let metadata = fs::symlink_metadata(&file)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(fail(
            "adoption.index",
            format!(
                "Probierz adoption index is not a regular file: {}",
                file.display()
            ),
        ));
    }
    let document: Value = serde_json::from_slice(&fs::read(&file)?)?;
    if document.get("schema").and_then(Value::as_str) != Some(INDEX_SCHEMA)
        || !document.get("sources").is_some_and(Value::is_array)
    {
        return Err(fail(
            "adoption.index",
            format!("unsupported Probierz adoption index: {}", file.display()),
        ));
    }
    validate_index_records(&document, project_root, &file)?;
    serde_json::from_value(document).map_err(|_| {
        fail(
            "adoption.index",
            format!(
                "invalid Probierz adoption source record: {}",
                file.display()
            ),
        )
    })
}

fn validate_index_records(
    document: &Value,
    project_root: &Path,
    file: &Path,
) -> Result<(), Failure> {
    for source in document["sources"].as_array().expect("checked") {
        let valid_source = source
            .get("sourceKey")
            .and_then(Value::as_str)
            .is_some_and(is_sha256)
            && source.get("sourceRoot").is_some_and(Value::is_string)
            && source.get("files").is_some_and(Value::is_array);
        if !valid_source {
            return Err(fail(
                "adoption.index",
                format!(
                    "invalid Probierz adoption source record: {}",
                    file.display()
                ),
            ));
        }
        for retained in source["files"].as_array().expect("checked") {
            let valid_file = retained.get("path").is_some_and(Value::is_string)
                && retained
                    .get("sha256")
                    .and_then(Value::as_str)
                    .is_some_and(is_sha256)
                && retained.get("mode").and_then(Value::as_u64).is_some();
            if !valid_file {
                return Err(fail(
                    "adoption.index",
                    format!("invalid Probierz adoption file record: {}", file.display()),
                ));
            }
            absolute(
                project_root,
                retained["path"].as_str().expect("validated string"),
            )?;
        }
    }
    Ok(())
}

fn file_owners(index: &AdoptionIndex) -> HashMap<&str, Vec<&str>> {
    let mut owners: HashMap<&str, Vec<&str>> = HashMap::new();
    for source in &index.sources {
        for file in &source.files {
            owners
                .entry(file.path.as_str())
                .or_default()
                .push(source.source_key.as_str());
        }
    }
    owners
}

enum CurrentFile {
    Missing,
    Unsupported,
    Regular { sha256: String, mode: u32 },
}

impl CurrentFile {
    fn digest_value(&self) -> Value {
        match self {
            Self::Missing => Value::Null,
            Self::Unsupported => Value::String("unsupported".to_string()),
            Self::Regular { sha256, .. } => Value::String(sha256.clone()),
        }
    }
}

fn current_file(file: &Path) -> Result<CurrentFile, Failure> {
    let metadata = match fs::symlink_metadata(file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CurrentFile::Missing)
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(CurrentFile::Unsupported);
    }
    Ok(CurrentFile::Regular {
        sha256: sha256_file(file)?,
        mode: metadata_mode(&metadata),
    })
}

fn conflict(
    path: &str,
    reason: &'static str,
    existing_sha256: Value,
    incoming_sha256: Value,
) -> Conflict {
    Conflict {
        path: path.to_string(),
        reason,
        existing_sha256,
        incoming_sha256,
    }
}

fn result(
    status: &str,
    source_root: &str,
    definitions: &Definitions,
    counts: Counts,
    conflicts: &[Conflict],
) -> Value {
    json!({
        "schema": RESULT_SCHEMA,
        "status": status,
        "sourceRoot": source_root,
        "sourceDigest": definitions.source_digest,
        "applications": definitions.application_ids,
        "imported": counts.imported,
        "unchanged": counts.unchanged,
        "removed": counts.removed,
        "conflicting": conflicts.len(),
        "rejected": counts.rejected,
        "conflicts": conflicts.iter().map(|item| json!({
            "path": item.path,
            "reason": item.reason,
            "existingSha256": item.existing_sha256,
            "incomingSha256": item.incoming_sha256,
        })).collect::<Vec<_>>(),
        "skippedLocalState": definitions.skipped_local_state,
        "executedJourneys": false,
    })
}

fn apply_transaction(
    destination: &Path,
    planned: &[&DefinitionFile],
    removals: &[String],
    index: &AdoptionIndex,
) -> Result<(), Failure> {
    let transaction_id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let stage_root = destination.join(format!(".probierz-adoption-stage-{transaction_id}"));
    let backup_root = destination.join(format!(".probierz-adoption-backup-{transaction_id}"));
    let index_bytes = format!("{}\n", serde_json::to_string_pretty(index)?).into_bytes();
    let mut backed_up: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut placed: Vec<PathBuf> = Vec::new();

    let operation = (|| -> Result<(), Failure> {
        for file in planned {
            let staged_file = absolute(&stage_root, &file.relative)?;
            if let Some(parent) = staged_file.parent() {
                fs::create_dir_all(parent)?;
            }
            write_new(&staged_file, &file.bytes, file.mode)?;
            set_mode(&staged_file, file.mode)?;
        }
        let staged_index = absolute(&stage_root, INDEX_RELATIVE_PATH)?;
        if let Some(parent) = staged_index.parent() {
            fs::create_dir_all(parent)?;
        }
        write_private(&staged_index, &index_bytes)?;
        set_mode(&staged_index, 0o600)?;

        for relative in removals {
            let target = absolute(destination, relative)?;
            let backup = absolute(&backup_root, relative)?;
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&target, &backup)?;
            backed_up.push((target, backup));
        }
        for relative in planned
            .iter()
            .map(|file| file.relative.as_str())
            .chain(std::iter::once(INDEX_RELATIVE_PATH))
        {
            let target = absolute(destination, relative)?;
            let staged_file = absolute(&stage_root, relative)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            if target.exists() {
                let backup = absolute(&backup_root, relative)?;
                if let Some(parent) = backup.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(&target, &backup)?;
                backed_up.push((target.clone(), backup));
            }
            fs::rename(staged_file, &target)?;
            placed.push(target);
        }
        Ok(())
    })();

    if operation.is_err() {
        for target in placed.iter().rev() {
            let _ = fs::remove_file(target);
        }
        for (target, backup) in backed_up.iter().rev() {
            if backup.exists() {
                if let Some(parent) = target.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::rename(backup, target);
            }
        }
    }
    let _ = fs::remove_dir_all(&stage_root);
    let _ = fs::remove_dir_all(&backup_root);
    operation
}

fn repository_root(value: &Path, label: &str) -> Result<PathBuf, Failure> {
    let canonical = fs::canonicalize(value).map_err(|_| {
        fail(
            "adoption.repository",
            format!("{label} is not an existing directory: {}", value.display()),
        )
    })?;
    let metadata = fs::symlink_metadata(&canonical)?;
    if !metadata.is_dir() {
        return Err(fail(
            "adoption.repository",
            format!("{label} is not a directory: {}", canonical.display()),
        ));
    }
    let git = canonical.join(".git");
    let git_metadata = match fs::symlink_metadata(&git) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Err(fail(
                "adoption.repository",
                format!("{label} is not a Git repository: {}", canonical.display()),
            ))
        }
    };
    if git_metadata.file_type().is_symlink() || (!git_metadata.is_dir() && !git_metadata.is_file())
    {
        return Err(fail(
            "adoption.repository",
            format!("{label} has an unsupported .git entry: {}", git.display()),
        ));
    }
    Ok(canonical)
}

fn absolute(root: &Path, relative: &str) -> Result<PathBuf, Failure> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(fail(
            "adoption.path",
            format!("project definition path escapes its repository: {relative}"),
        ));
    }
    Ok(root.join(path))
}

fn target_package(target: &str) -> Option<&'static str> {
    TARGET_PACKAGES
        .iter()
        .find_map(|(known, package)| (*known == target).then_some(*package))
}

fn matches_declared_spec(pattern: &str, relative_to_package: &str) -> bool {
    let trimmed = pattern.strip_prefix("./").unwrap_or(pattern);
    let normalized_storage;
    let normalized = if std::path::MAIN_SEPARATOR == '\\' {
        normalized_storage = trimmed.replace('\\', "/");
        normalized_storage.as_str()
    } else {
        trimmed
    };
    let compared = if normalized.contains('/') {
        relative_to_package
    } else {
        relative_to_package
            .rsplit('/')
            .next()
            .unwrap_or(relative_to_package)
    };
    wildcard_match(normalized, compared)
}

fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    let pattern = pattern.as_bytes();
    let candidate = candidate.as_bytes();
    let (mut pattern_index, mut candidate_index) = (0usize, 0usize);
    let (mut star, mut retry_candidate) = (None, 0usize);
    while candidate_index < candidate.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == candidate[candidate_index] {
            pattern_index += 1;
            candidate_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star = Some(pattern_index);
            pattern_index += 1;
            retry_candidate = candidate_index;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            retry_candidate += 1;
            candidate_index = retry_candidate;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<(), Failure> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(unix)]
fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    if metadata.permissions().readonly() {
        0o444
    } else {
        0o666
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(mode & 0o200 == 0);
    fs::set_permissions(path, permissions)
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn sha256_file(path: &Path) -> Result<String, Failure> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn os_text(value: &OsStr) -> &str {
    value.to_str().unwrap_or_default()
}

fn progress_file() -> Result<PathBuf, Failure> {
    let state_root = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| PathBuf::from(value.trim()))
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .map(|home| home.join(".local/state"))
        })
        .ok_or_else(|| fail("onboarding.state", "home directory is unavailable"))?;
    Ok(state_root.join("probierz/onboarding.json"))
}

fn read_progress() -> Option<Value> {
    let file = progress_file().ok()?;
    serde_json::from_slice(&fs::read(file).ok()?).ok()
}

fn write_progress(progress: &Value) -> Result<(), Failure> {
    let file = progress_file()?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = format!("{}\n", serde_json::to_string_pretty(progress)?);
    write_private(&file, body.as_bytes())?;
    Ok(())
}

fn write_initial_progress(definition: &Value) -> Result<(), Failure> {
    write_progress(&json!({
        "product_id": definition["product_id"],
        "journey_id": definition["journey_id"],
        "journey_version": definition["journey_version"],
        "status": "in_progress",
        "evidence": {},
        "started_at": now_iso(),
    }))
}

fn ordered_screens(definition: &Value) -> Vec<&Value> {
    let Some(screens) = definition.get("screens").and_then(Value::as_array) else {
        return Vec::new();
    };
    let by_id: HashMap<&str, &Value> = screens
        .iter()
        .filter_map(|screen| {
            screen
                .get("screen_id")
                .and_then(Value::as_str)
                .map(|id| (id, screen))
        })
        .collect();
    let mut ordered = Vec::new();
    let mut current = definition
        .get("entry_screen_id")
        .and_then(Value::as_str)
        .and_then(|id| by_id.get(id).copied());
    while let Some(screen) = current {
        if ordered
            .iter()
            .any(|seen: &&Value| std::ptr::eq(*seen, screen))
        {
            break;
        }
        ordered.push(screen);
        current = screen
            .get("transitions")
            .and_then(Value::as_array)
            .and_then(|transitions| {
                transitions.iter().min_by_key(|transition| {
                    transition
                        .get("priority")
                        .and_then(Value::as_i64)
                        .unwrap_or(i64::MAX)
                })
            })
            .and_then(|transition| transition.get("next_screen_id"))
            .and_then(Value::as_str)
            .and_then(|id| by_id.get(id).copied());
    }
    ordered
}

fn onboarding_definition() -> Value {
    json!({
        "schema_version": 1,
        "product_id": "probierz",
        "journey_id": "first-use",
        "journey_version": "2026-09-03.2",
        "entry_screen_id": "adopt-existing-project",
        "first_success_fact": "passing_quality_evidence_written",
        "published_at": "2026-09-03T00:00:00Z",
        "source_revision": "probierz-first-use-2026-09-03.2",
        "screens": [
            {
                "screen_id": "adopt-existing-project",
                "screen_kind": "import",
                "title_key": "probierz.first_use.adopt.title",
                "body_key": "probierz.first_use.adopt.body",
                "required": false,
                "actions": ["import", "skip"],
                "transitions": [{
                    "next_screen_id": "choose-one-journey",
                    "reason_code": "project_adopted_or_skipped",
                    "priority": 10
                }],
                "presentation": {
                    "title": "Bring your existing Probierz project",
                    "body": "Choose another Probierz repository to adopt its validated apps/<appId>/probierz.yaml manifests and established package spec directories. Probierz preserves the definitions, reports every conflict, and does not run a journey. Skip keeps this project empty and usable."
                }
            },
            {
                "screen_id": "choose-one-journey",
                "screen_kind": "explanation",
                "title_key": "probierz.first_use.choose.title",
                "body_key": "probierz.first_use.choose.body",
                "required": true,
                "actions": ["advance"],
                "transitions": [{
                    "next_screen_id": "read-the-evidence",
                    "reason_code": "journey_selected",
                    "priority": 10
                }],
                "presentation": {
                    "title": "Start with one declared journey",
                    "body": "Probierz runs evidence for a product, surface and user journey declared in an application manifest. Begin with `probierz apps`, then inspect one registration with `probierz app APP_ID`; its surface names the target and spec you can run instead of guessing either."
                }
            },
            {
                "screen_id": "read-the-evidence",
                "screen_kind": "explanation",
                "title_key": "probierz.first_use.evidence.title",
                "body_key": "probierz.first_use.evidence.body",
                "required": true,
                "actions": ["advance"],
                "transitions": [{
                    "next_screen_id": "receipts-follow-runs",
                    "reason_code": "evidence_model_explained",
                    "priority": 10
                }],
                "presentation": {
                    "title": "A completed run leaves quality evidence",
                    "body": "The first durable result is a run manifest, not a claim that a suite passed. Probierz binds the report, analysis, source and build identities, conditions and artifact hashes into that record, then reports a pass or fail without averaging failures away."
                }
            },
            {
                "screen_id": "receipts-follow-runs",
                "screen_kind": "explanation",
                "title_key": "probierz.first_use.receipts.title",
                "body_key": "probierz.first_use.receipts.body",
                "required": true,
                "actions": ["advance"],
                "transitions": [{
                    "next_screen_id": "produce-evidence",
                    "reason_code": "receipt_sequence_explained",
                    "priority": 10
                }],
                "presentation": {
                    "title": "Release receipts come after recorded runs",
                    "body": "A release gate consumes exact run IDs and identities. Once the required journeys have qualifying evidence, `probierz receipt` signs the resulting verdict for a release; it cannot replace the underlying run records or turn missing evidence green."
                }
            },
            {
                "screen_id": "produce-evidence",
                "screen_kind": "guided_query",
                "title_key": "probierz.first_use.run.title",
                "body_key": "probierz.first_use.run.body",
                "required": true,
                "completion_evidence": {
                    "kind": "fact",
                    "fact": "passing_quality_evidence_written",
                    "operator": "eq",
                    "value": true
                },
                "actions": ["run"],
                "transitions": [],
                "presentation": {
                    "title": "Produce your first evidence record",
                    "body": "Run one declared spec on its target with `probierz run TARGET --app APP_ID --spec SPEC`. When the command succeeds and Probierz writes a passing evidence block into the run manifest, this journey is complete. A failed run remains honest, actionable evidence, but the release gate stays red and this first-success step stays open.",
                    "command": "probierz run TARGET --app APP_ID --spec SPEC",
                    "result": "A passing JSON run result with its run ID, evidence checks and run-manifest path"
                }
            }
        ],
        "analytics_contract": {
            "contract_version": "1",
            "surface": "cli_first_use",
            "exposure_event": "onboarding_step_viewed",
            "primary_action_event": "onboarding_step_completed",
            "completion_event": "onboarding_completed",
            "first_success_event": "onboarding_first_success_observed"
        }
    })
}
