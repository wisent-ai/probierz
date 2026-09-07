//! Merge and release gates.
//!
//! A verdict names the exact harness, application source, builds, journeys and
//! artifacts it judged.  A gate that cannot prove one of those identities is
//! blocked rather than weakened.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};

use chrono::{SecondsFormat, Utc};
use clap::Args;
use serde_json::{Map, Value};
use serde_yaml::Value as Yaml;
use sha2::{Digest, Sha256};

use crate::failure::{print_json, Answer, Failure};
use crate::manifest;

const ZERO_SHA: &str = "0000000000000000000000000000000000000000";
const MANAGED_MARKER: &str = "# managed-by: probierz-prepush-gate";

#[derive(Debug, Clone, Args)]
pub struct GateArgs {
    pub app_id: String,
    pub mode: String,
    pub expected_harness_sha: String,
    #[arg(long = "source-sha")]
    pub expected_source_sha: Option<String>,
    #[arg(long)]
    pub runs: Option<String>,
    #[arg(long)]
    pub release: Option<String>,
    #[arg(long)]
    pub receipt: Option<PathBuf>,
    #[arg(long = "public-key")]
    pub public_key: Option<PathBuf>,
    #[arg(long)]
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct PrepushArgs {
    #[arg(long)]
    pub repo: Option<PathBuf>,
    #[arg(long = "app")]
    pub app_id: Option<String>,
    #[arg(long)]
    pub base: Option<String>,
    #[arg(long)]
    pub head: Option<String>,
    #[arg(long = "ci")]
    pub run_ci: bool,
    #[arg(long, hide = true)]
    pub hook: bool,
    #[arg(long, hide = true)]
    pub json: bool,
    #[arg(long = "ci-arg", hide = true)]
    pub ci_args: Vec<String>,
}

#[derive(Debug, Clone, Args)]
pub struct InstallArgs {
    pub app_id: String,
    #[arg(long)]
    pub repo: Option<PathBuf>,
}

fn object(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in entries {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn strings(values: &[String]) -> Value {
    Value::Array(values.iter().cloned().map(Value::String).collect())
}

fn config_file(manifest: &manifest::Manifest) -> PathBuf {
    manifest
        .file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("gates.json")
}

fn default_config(app_id: &str) -> Value {
    object([
        ("schemaVersion", Value::from(2)),
        ("appId", Value::String(app_id.to_string())),
        (
            "modes",
            object([
                (
                    "pull-request",
                    object([("enforcement", Value::String("pending-green".to_string()))]),
                ),
                (
                    "release",
                    object([("enforcement", Value::String("pending-green".to_string()))]),
                ),
            ]),
        ),
    ])
}

fn gate_status_value(harness: &Path, app_id: &str) -> Result<Value, Failure> {
    let app = manifest::load(harness, app_id)?;
    let file = config_file(&app);
    let exists = file.exists();
    let mut config = if exists {
        serde_json::from_str::<Value>(&fs::read_to_string(&file)?)?
    } else {
        default_config(app_id)
    };
    let map = config.as_object_mut().ok_or_else(|| {
        Failure::config(
            "gate.status",
            format!("{} does not contain a gate object", file.display()),
        )
    })?;
    map.insert(
        "file".to_string(),
        Value::String(file.to_string_lossy().into_owned()),
    );
    map.insert("exists".to_string(), Value::Bool(exists));
    Ok(config)
}

pub fn status(harness: &Path, app_id: &str) -> Answer {
    print_json(&gate_status_value(harness, app_id)?)
}

fn yaml_get<'a>(value: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    value.as_mapping()?.get(&Yaml::String(key.to_string()))
}

fn yaml_string(value: Option<&Yaml>) -> Option<String> {
    value.and_then(Yaml::as_str).map(str::to_string)
}

fn yaml_strings(value: Option<&Yaml>) -> Vec<String> {
    value
        .and_then(Yaml::as_sequence)
        .map(|list| {
            list.iter()
                .filter_map(Yaml::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn yaml_bool(value: Option<&Yaml>) -> bool {
    value.and_then(Yaml::as_bool).unwrap_or(false)
}

fn yaml_js_string(value: &Yaml) -> String {
    match value {
        Yaml::Null => "null".to_string(),
        Yaml::Bool(flag) => flag.to_string(),
        Yaml::Number(number) => number.to_string(),
        Yaml::String(text) => text.clone(),
        other => serde_json::to_string(&serde_json::to_value(other).unwrap_or(Value::Null))
            .unwrap_or_default(),
    }
}

fn evidence_rank(level: &str) -> Option<i32> {
    match level {
        "E0" => Some(0),
        "E1" => Some(1),
        "E2" => Some(2),
        "E3" => Some(3),
        _ => None,
    }
}

fn property<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.as_object()?.get(name)
}

fn string_property(value: &Value, name: &str) -> Option<String> {
    property(value, name)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number
            .as_f64()
            .map(|number| number != 0.0 && !number.is_nan())
            .unwrap_or(false),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

fn value_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn value_strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
struct Run {
    manifest_path: PathBuf,
    run_id: String,
    kind: String,
    target: String,
    spec: Value,
    status: String,
    started_at: Value,
    completed_at: Value,
    harness: Value,
    source: Value,
    build: Value,
    journeys: Vec<String>,
    device: Value,
    conditions: Value,
    evidence: Value,
    artifacts: Vec<Value>,
    protection: Value,
    analysis_path: Option<String>,
}

fn normalized_status(document: &Value) -> String {
    match string_property(document, "status").as_deref() {
        Some("passed") | Some("executed") => "passed".to_string(),
        Some("blocked") => "blocked".to_string(),
        Some("canceled") => "canceled".to_string(),
        Some("failed") => "failed".to_string(),
        _ if truthy(property(document, "completedAt")) => "failed".to_string(),
        _ => "incomplete".to_string(),
    }
}

fn run_from_file(file: &Path) -> Result<Option<Run>, String> {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(_) => return Ok(None),
    };
    let document: Value = match serde_json::from_str(&text) {
        Ok(document) => document,
        Err(_) => return Ok(None),
    };
    let directory = file.parent().unwrap_or_else(|| Path::new("."));
    let analysis_path = string_property(&document, "analysisPath");
    let journeys = property(&document, "appManifest")
        .and_then(|value| property(value, "journeys"))
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let run_id = string_property(&document, "runId").unwrap_or_default();
    let target = string_property(&document, "target").unwrap_or_default();
    Ok(Some(Run {
        manifest_path: directory.join("run-manifest.json"),
        run_id,
        kind: string_property(&document, "kind").unwrap_or_else(|| "adhoc".to_string()),
        target,
        spec: property(&document, "spec").cloned().unwrap_or(Value::Null),
        status: normalized_status(&document),
        started_at: property(&document, "startedAt")
            .cloned()
            .unwrap_or(Value::Null),
        completed_at: property(&document, "completedAt")
            .cloned()
            .unwrap_or(Value::Null),
        harness: property(&document, "harness")
            .cloned()
            .unwrap_or(Value::Null),
        source: property(&document, "source")
            .cloned()
            .unwrap_or(Value::Null),
        build: property(&document, "build").cloned().unwrap_or(Value::Null),
        journeys,
        device: property(&document, "device")
            .cloned()
            .unwrap_or(Value::Null),
        conditions: property(&document, "conditions")
            .cloned()
            .unwrap_or_else(|| object([])),
        evidence: property(&document, "evidence")
            .cloned()
            .unwrap_or(Value::Null),
        artifacts: value_array(property(&document, "artifacts")),
        protection: property(&document, "protection")
            .cloned()
            .unwrap_or(Value::Null),
        analysis_path,
    }))
}

fn manifests_below(root: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    let entries = fs::read_dir(root).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir() {
            manifests_below(&entry.path(), found)?;
        } else if kind.is_file() && entry.file_name() == "run-manifest.json" {
            found.push(entry.path());
        }
    }
    Ok(())
}

fn all_runs(harness: &Path, app_id: &str) -> Result<Vec<Run>, String> {
    let mut files = Vec::new();
    manifests_below(&harness.join("test-results").join(app_id), &mut files)?;
    let mut runs = Vec::new();
    for file in files {
        if let Some(run) = run_from_file(&file)? {
            runs.push(run);
        }
    }
    Ok(runs)
}

fn get_run(harness: &Path, app_id: &str, run_id: &str) -> Result<Run, String> {
    all_runs(harness, app_id)?
        .into_iter()
        .find(|run| run.run_id == run_id)
        .ok_or_else(|| format!("run not found for {app_id}: {run_id}"))
}

fn evidence_level(run: &Run) -> &'static str {
    if run.status != "passed" {
        return "E0";
    }
    let record = truthy(property(&run.conditions, "record"));
    let report = truthy(property(&run.evidence, "report"));
    let analysis = truthy(property(&run.evidence, "analysis"));
    let capture = truthy(property(&run.evidence, "capturePresent"));
    if record && report && analysis && capture {
        "E3"
    } else {
        "E2"
    }
}

fn sha256_file(file: &Path) -> Result<String, String> {
    let mut input = File::open(file).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(hex::encode(hash.finalize()))
}

fn normalize_absolute(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let members = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        canonical(&map[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{members}}}")
        }
        primitive => serde_json::to_string(primitive).unwrap_or_else(|_| "null".to_string()),
    }
}

fn js_display(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn js_strict_optional_string(actual: Option<&Value>, expected: Option<&str>) -> bool {
    match expected {
        Some(expected) => actual.and_then(Value::as_str) == Some(expected),
        None => actual.is_none(),
    }
}

fn same_set(left: &[String], right: &[String]) -> bool {
    let a: BTreeSet<&String> = left.iter().collect();
    let b: BTreeSet<&String> = right.iter().collect();
    a == b
}

fn yaml_mapping<'a>(value: Option<&'a Yaml>) -> Option<&'a serde_yaml::Mapping> {
    value.and_then(Yaml::as_mapping)
}

fn matrix_cells(app: &manifest::Manifest, profile: &str) -> Result<Vec<Value>, String> {
    let matrix = yaml_mapping(yaml_get(&app.document, "matrix"));
    let policy = matrix
        .and_then(|mapping| mapping.get(&Yaml::String(profile.to_string())))
        .ok_or_else(|| format!("app {} has no {profile} matrix", app.app_id))?;
    let surfaces = yaml_mapping(yaml_get(&app.document, "surfaces"))
        .ok_or_else(|| "manifest has no surfaces".to_string())?;
    let targets = {
        let configured = yaml_strings(yaml_get(policy, "targets"));
        if configured.is_empty() {
            surfaces
                .keys()
                .filter_map(Yaml::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        } else {
            configured
        }
    };
    let base_dimensions = yaml_mapping(yaml_get(policy, "dimensions"));
    let per_surface = yaml_mapping(yaml_get(policy, "surfaces"));
    let mut sorted_targets = targets;
    sorted_targets.sort();
    let mut cells = Vec::new();
    for target in sorted_targets {
        let surface = surfaces
            .get(&Yaml::String(target.clone()))
            .ok_or_else(|| format!("matrix {profile} references unknown target: {target}"))?;
        let mut dimensions: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if let Some(mapping) = base_dimensions {
            for (name, values) in mapping {
                if let Some(name) = name.as_str() {
                    dimensions.insert(
                        name.to_string(),
                        values
                            .as_sequence()
                            .map(|items| items.iter().map(yaml_js_string).collect())
                            .unwrap_or_default(),
                    );
                }
            }
        }
        if let Some(mapping) = per_surface
            .and_then(|all| all.get(&Yaml::String(target.clone())))
            .and_then(|entry| yaml_mapping(yaml_get(entry, "dimensions")))
        {
            for (name, values) in mapping {
                if let Some(name) = name.as_str() {
                    dimensions.insert(
                        name.to_string(),
                        values
                            .as_sequence()
                            .map(|items| items.iter().map(yaml_js_string).collect())
                            .unwrap_or_default(),
                    );
                }
            }
        }
        let mut expanded: Vec<Map<String, Value>> = vec![Map::new()];
        for (name, values) in dimensions {
            let mut next = Vec::new();
            for existing in &expanded {
                for value in &values {
                    let mut cell = existing.clone();
                    cell.insert(name.clone(), Value::String(value.clone()));
                    next.push(cell);
                }
            }
            expanded = next;
        }
        for axes in expanded {
            let mut conditions = Map::new();
            if let Some(surface_conditions) = yaml_mapping(yaml_get(surface, "conditions")) {
                for (name, value) in surface_conditions {
                    if let Some(name) = name.as_str() {
                        conditions.insert(
                            name.to_string(),
                            serde_json::to_value(value).unwrap_or(Value::Null),
                        );
                    }
                }
            }
            for (name, value) in &axes {
                conditions.insert(name.clone(), value.clone());
            }
            let stable = object([
                (
                    "env",
                    Value::Object({
                        let mut sorted = Map::new();
                        let mut names: Vec<_> = conditions.keys().cloned().collect();
                        names.sort();
                        for name in names {
                            sorted.insert(name.clone(), conditions[&name].clone());
                        }
                        sorted
                    }),
                ),
                ("target", Value::String(target.clone())),
            ]);
            let cell_id = hex::encode(Sha256::digest(
                serde_json::to_vec(&stable).map_err(|error| error.to_string())?,
            ))[..16]
                .to_string();
            cells.push(object([
                ("cellId", Value::String(cell_id)),
                ("target", Value::String(target.clone())),
                ("axes", Value::Object(axes)),
            ]));
        }
    }
    let max_cells = yaml_get(policy, "maxCells")
        .and_then(Yaml::as_u64)
        .unwrap_or(128) as usize;
    if cells.len() > max_cells {
        return Err(format!(
            "matrix {profile} expands to {} cells (max {max_cells})",
            cells.len()
        ));
    }
    Ok(cells)
}

fn matrix_coverage(app: &manifest::Manifest, profile: &str, runs: &[Run]) -> Result<Value, String> {
    let cells = matrix_cells(app, profile)?;
    let mut remaining: Vec<&Run> = runs.iter().collect();
    let mut missing = Vec::new();
    for cell in &cells {
        let target = string_property(cell, "target").unwrap_or_default();
        let axes = property(cell, "axes")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let index = remaining.iter().position(|run| {
            run.target == target
                && axes.iter().all(|(name, wanted)| {
                    property(&run.conditions, name)
                        .map(|actual| js_display(Some(actual)) == js_display(Some(wanted)))
                        .unwrap_or_else(|| js_display(None) == js_display(Some(wanted)))
                })
        });
        if let Some(index) = index {
            remaining.remove(index);
        } else {
            missing.push(cell.clone());
        }
    }
    Ok(object([
        ("profile", Value::String(profile.to_string())),
        ("expected", Value::from(cells.len())),
        ("matched", Value::from(cells.len() - missing.len())),
        ("missing", Value::Array(missing)),
        (
            "extraRunIds",
            Value::Array(
                remaining
                    .into_iter()
                    .map(|run| Value::String(run.run_id.clone()))
                    .collect(),
            ),
        ),
    ]))
}

fn receipt_run_value(run: &Run) -> Value {
    object([
        ("runId", Value::String(run.run_id.clone())),
        ("target", Value::String(run.target.clone())),
        ("spec", run.spec.clone()),
        ("journeys", strings(&run.journeys)),
        ("status", Value::String(run.status.clone())),
        ("kind", Value::String(run.kind.clone())),
        ("harness", run.harness.clone()),
        ("source", run.source.clone()),
        ("build", run.build.clone()),
        ("device", run.device.clone()),
        ("startedAt", run.started_at.clone()),
        ("completedAt", run.completed_at.clone()),
        ("conditions", run.conditions.clone()),
        ("evidence", run.evidence.clone()),
        ("protection", run.protection.clone()),
        ("artifacts", Value::Array(run.artifacts.clone())),
        (
            "manifestPath",
            Value::String(run.manifest_path.to_string_lossy().into_owned()),
        ),
        (
            "analysisPath",
            run.analysis_path
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        ),
    ])
}

fn mode_policy<'a>(app: &'a manifest::Manifest, mode: &str) -> Option<&'a Yaml> {
    yaml_get(
        &app.document,
        if mode == "release" {
            "releasePolicy"
        } else {
            "pullRequestPolicy"
        },
    )
}

fn evaluate_value(harness: &Path, args: &GateArgs) -> Result<Value, Failure> {
    if args.app_id.is_empty() || !matches!(args.mode.as_str(), "pull-request" | "release") {
        return Err(Failure::invalid(
            "gate.evaluate",
            "gate needs an app ID and pull-request or release mode",
        ));
    }
    let app = manifest::load(harness, &args.app_id)?;
    let empty_policy = Yaml::Mapping(Default::default());
    let policy = mode_policy(&app, &args.mode).unwrap_or(&empty_policy);
    let minimum_evidence =
        yaml_string(yaml_get(policy, "minimumEvidence")).unwrap_or_else(|| "E3".to_string());
    let required_rank = evidence_rank(&minimum_evidence).ok_or_else(|| {
        Failure::config(
            "gate.evaluate",
            format!("unsupported gate evidence level: {minimum_evidence}"),
        )
    })?;
    let required_targets = yaml_strings(yaml_get(policy, "requiredTargets"));
    let required_journeys = yaml_strings(yaml_get(policy, "requiredJourneys"));
    let matrix_profile = yaml_string(yaml_get(policy, "requiredMatrixProfile"));
    let require_protected = yaml_bool(yaml_get(policy, "requireProtectedArtifacts"));
    let require_secret_scan = yaml_bool(yaml_get(policy, "requireSecretScan"));
    let run_ids: Vec<String> = args
        .runs
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    let mut errors = Vec::new();
    if args.expected_harness_sha.is_empty() {
        errors.push("expected harness source SHA-256 is required".to_string());
    }
    if args.expected_source_sha.as_deref().unwrap_or("").is_empty() {
        errors.push("expected app source SHA-256 is required".to_string());
    }
    if run_ids.is_empty() {
        errors.push("at least one run ID is required".to_string());
    }
    if run_ids.iter().collect::<BTreeSet<_>>().len() != run_ids.len() {
        errors.push("run IDs must be unique".to_string());
    }
    let mut seen = BTreeSet::new();
    let mut runs = Vec::new();
    for run_id in &run_ids {
        if !seen.insert(run_id.clone()) {
            continue;
        }
        match get_run(harness, &args.app_id, run_id) {
            Ok(run) => runs.push(run),
            Err(error) => errors.push(error),
        }
    }
    let expected_source = args.expected_source_sha.as_deref();
    let mut bundle_hashes: HashMap<String, String> = HashMap::new();
    for run in &runs {
        if run.status != "passed" {
            errors.push(format!("{}: status is {}", run.run_id, run.status));
        }
        let harness_sha = string_property(&run.harness, "sha256");
        let harness_git = string_property(&run.harness, "gitSha");
        let harness_worktree = string_property(&run.harness, "worktreeSha256");
        if harness_sha.as_deref().unwrap_or("").is_empty()
            || harness_git.as_deref().unwrap_or("").is_empty()
            || harness_worktree.as_deref().unwrap_or("").is_empty()
        {
            errors.push(format!(
                "{}: complete harness source identity is missing",
                run.run_id
            ));
        } else if harness_sha.as_deref() != Some(args.expected_harness_sha.as_str()) {
            errors.push(format!(
                "{}: harness source {} does not match {}",
                run.run_id,
                harness_sha.unwrap_or_default(),
                args.expected_harness_sha
            ));
        }
        if string_property(&run.build, "sha256")
            .as_deref()
            .unwrap_or("")
            .is_empty()
        {
            errors.push(format!("{}: exact build hash is missing", run.run_id));
        }
        if evidence_rank(evidence_level(run)).unwrap_or(-1) < required_rank {
            errors.push(format!(
                "{}: {} is below {}",
                run.run_id,
                evidence_level(run),
                minimum_evidence
            ));
        }
        let source_sha = string_property(&run.source, "sha256");
        let repositories = property(&run.source, "repositories").and_then(Value::as_array);
        let repositories_complete = repositories
            .map(|items| {
                items.iter().all(|repository| {
                    !string_property(repository, "gitSha")
                        .as_deref()
                        .unwrap_or("")
                        .is_empty()
                        && !string_property(repository, "worktreeSha256")
                            .as_deref()
                            .unwrap_or("")
                            .is_empty()
                })
            })
            .unwrap_or(false);
        if source_sha.as_deref().unwrap_or("").is_empty() || !repositories_complete {
            errors.push(format!(
                "{}: complete app source identity is missing",
                run.run_id
            ));
        }
        if let Some(expected_source) = expected_source.filter(|value| !value.is_empty()) {
            if source_sha.as_deref() != Some(expected_source) {
                errors.push(format!(
                    "{}: app source {} does not match {expected_source}",
                    run.run_id,
                    source_sha.unwrap_or_else(|| "missing".to_string())
                ));
            }
        }
        if required_rank >= 3
            && (run.artifacts.is_empty()
                || run.artifacts.iter().any(|artifact| {
                    string_property(artifact, "sha256")
                        .as_deref()
                        .unwrap_or("")
                        .is_empty()
                }))
        {
            errors.push(format!("{}: E3 artifact hashes are incomplete", run.run_id));
        }
        if !truthy(property(&run.protection, "plaintextRemoved")) {
            let artifact_root = run.manifest_path.parent().unwrap_or_else(|| Path::new("."));
            for artifact in &run.artifacts {
                let relative = string_property(artifact, "file").unwrap_or_default();
                let file = normalize_absolute(&artifact_root.join(&relative));
                let normalized_root = normalize_absolute(artifact_root);
                if file != normalized_root && !file.starts_with(&normalized_root) {
                    errors.push(format!(
                        "{}: artifact path escapes its run: {relative}",
                        run.run_id
                    ));
                } else if !file.exists() {
                    errors.push(format!("{}: artifact is missing: {relative}", run.run_id));
                } else {
                    match sha256_file(&file) {
                        Ok(actual)
                            if Some(actual.as_str())
                                != string_property(artifact, "sha256").as_deref() =>
                        {
                            errors.push(format!(
                                "{}: artifact hash mismatch: {relative}",
                                run.run_id
                            ))
                        }
                        Ok(_) => {}
                        Err(error) => errors.push(format!(
                            "{}: artifact cannot be hashed: {relative} ({error})",
                            run.run_id
                        )),
                    }
                }
            }
        }
        if run.kind != args.mode {
            errors.push(format!(
                "{}: run kind {} is not {}",
                run.run_id, run.kind, args.mode
            ));
        }
        if let Some(release) = args.release.as_deref().filter(|value| !value.is_empty()) {
            if args.mode == "release"
                && string_property(&run.conditions, "PROBIERZ_RELEASE").as_deref() != Some(release)
            {
                errors.push(format!(
                    "{}: release condition does not match {release}",
                    run.run_id
                ));
            }
        }
        if require_protected || truthy(property(&run.protection, "plaintextRemoved")) {
            let protected_file = string_property(&run.protection, "file");
            if !truthy(property(&run.protection, "plaintextRemoved"))
                || protected_file.as_deref().unwrap_or("").is_empty()
                || !protected_file
                    .as_deref()
                    .map(Path::new)
                    .map(Path::exists)
                    .unwrap_or(false)
            {
                errors.push(format!(
                    "{}: encrypted-at-rest artifact bundle is missing",
                    run.run_id
                ));
            } else if let Some(file) = protected_file {
                match sha256_file(Path::new(&file)) {
                    Ok(hash) => {
                        bundle_hashes.insert(run.run_id.clone(), hash.clone());
                        if string_property(&run.protection, "sha256").as_deref()
                            != Some(hash.as_str())
                        {
                            errors.push(format!(
                                "{}: encrypted bundle hash does not match its manifest",
                                run.run_id
                            ));
                        }
                    }
                    Err(error) => errors.push(format!(
                        "{}: encrypted bundle cannot be hashed: {error}",
                        run.run_id
                    )),
                }
            }
        }
        if require_secret_scan
            && !truthy(
                property(&run.protection, "secretScan").and_then(|scan| property(scan, "passed")),
            )
        {
            errors.push(format!(
                "{}: passing pre-upload secret scan is missing",
                run.run_id
            ));
        }
    }
    let source_hashes: BTreeSet<String> = runs
        .iter()
        .filter_map(|run| string_property(&run.source, "sha256"))
        .filter(|value| !value.is_empty())
        .collect();
    if !runs.is_empty() && source_hashes.len() != 1 {
        errors.push(format!(
            "runs do not identify one exact app source ({} source hashes)",
            source_hashes.len()
        ));
    }
    let mut builds = Map::new();
    for run in &runs {
        let Some(hash) = string_property(&run.build, "sha256").filter(|value| !value.is_empty())
        else {
            continue;
        };
        if let Some(existing) = builds.get(&run.target).and_then(Value::as_str) {
            if existing != hash {
                errors.push(format!(
                    "{}: runs do not identify one exact build",
                    run.target
                ));
            }
        } else {
            builds.insert(run.target.clone(), Value::String(hash));
        }
    }
    for target in &required_targets {
        if !runs.iter().any(|run| run.target == *target) {
            errors.push(format!("required target is missing: {target}"));
        }
    }
    for journey in &required_journeys {
        if !runs.iter().any(|run| run.journeys.contains(journey)) {
            errors.push(format!("required journey is missing: {journey}"));
        }
    }
    let matrix = if let Some(profile) = matrix_profile.as_deref() {
        let coverage = matrix_coverage(&app, profile, &runs)
            .map_err(|detail| Failure::config("gate.matrix", detail))?;
        let missing = property(&coverage, "missing")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        let extra = property(&coverage, "extraRunIds")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        if missing > 0 {
            errors.push(format!("{missing} required matrix cell(s) are missing"));
        }
        if extra > 0 {
            errors.push(format!("{extra} run(s) are outside the required matrix"));
        }
        coverage
    } else {
        Value::Null
    };
    let mut receipt = Value::Null;
    if args.mode == "release" {
        if args.release.as_deref().unwrap_or("").is_empty() {
            errors.push("release ID is required".to_string());
        }
        if let Some(receipt_file) = args.receipt.as_deref() {
            match crate::evidence::verify_receipt_value(
                receipt_file,
                args.public_key.as_deref(),
                args.fingerprint.as_deref(),
            ) {
                Ok(verified) => {
                    if !truthy(property(&verified, "valid")) {
                        errors.push(
                            "receipt signature, trust, or payload hash is invalid".to_string(),
                        );
                    }
                    if string_property(&verified, "appId").as_deref() != Some(args.app_id.as_str())
                    {
                        errors.push(format!(
                            "receipt app ID {} does not match {}",
                            js_display(property(&verified, "appId")),
                            args.app_id
                        ));
                    }
                    if !js_strict_optional_string(
                        property(&verified, "release"),
                        args.release.as_deref(),
                    ) {
                        errors.push(format!(
                            "receipt release {} does not match {}",
                            js_display(property(&verified, "release")),
                            args.release.as_deref().unwrap_or("undefined")
                        ));
                    }
                    if string_property(&verified, "expectedHarnessSha").as_deref()
                        != Some(args.expected_harness_sha.as_str())
                    {
                        errors.push("receipt harness source SHA-256 does not match".to_string());
                    }
                    if !js_strict_optional_string(
                        property(&verified, "expectedSourceSha"),
                        expected_source,
                    ) {
                        errors.push("receipt app source SHA-256 does not match".to_string());
                    }
                    if canonical(property(&verified, "builds").unwrap_or(&Value::Null))
                        != canonical(&Value::Object(builds.clone()))
                    {
                        errors.push("receipt build identities do not match".to_string());
                    }
                    let receipt_run_ids = value_strings(property(&verified, "runIds"));
                    if !same_set(&receipt_run_ids, &run_ids) {
                        errors.push("receipt run IDs do not match gate run IDs".to_string());
                    }
                    if !truthy(
                        property(&verified, "verdict")
                            .and_then(|verdict| property(verdict, "passed")),
                    ) {
                        errors.push("receipt verdict is not passed".to_string());
                    }
                    let signed_runs = value_array(property(&verified, "runs"));
                    let document = serde_json::to_value(&app.document)?;
                    for run in &runs {
                        let signed = signed_runs.iter().find(|candidate| {
                            string_property(candidate, "runId").as_deref()
                                == Some(run.run_id.as_str())
                        });
                        let local = crate::evidence::signed_receipt_run_value(
                            &receipt_run_value(run),
                            &document,
                        );
                        if signed.map(canonical) != Some(canonical(&local)) {
                            errors.push(format!(
                                "{}: local policy evidence differs from the signed receipt",
                                run.run_id
                            ));
                            continue;
                        }
                        if require_protected
                            && bundle_hashes.get(&run.run_id).map(String::as_str)
                                != signed
                                    .and_then(|item| property(item, "protection"))
                                    .and_then(|item| string_property(item, "sha256"))
                                    .as_deref()
                        {
                            errors.push(format!(
                                "{}: encrypted bundle does not match the signed receipt",
                                run.run_id
                            ));
                        }
                    }
                    receipt = verified;
                }
                Err(error) => errors.push(format!("receipt verification failed: {}", error.detail)),
            }
        } else {
            errors.push("signed receipt is required".to_string());
        }
    }
    let harness_matches = !runs.is_empty()
        && runs.iter().all(|run| {
            string_property(&run.harness, "sha256").as_deref()
                == Some(args.expected_harness_sha.as_str())
        });
    let source_sha = if source_hashes.len() == 1 {
        source_hashes
            .iter()
            .next()
            .cloned()
            .map(Value::String)
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let mut levels = Map::new();
    for run in &runs {
        levels.insert(
            run.run_id.clone(),
            Value::String(evidence_level(run).to_string()),
        );
    }
    let result = object([
        ("schemaVersion", Value::from(2)),
        ("appId", Value::String(args.app_id.clone())),
        ("mode", Value::String(args.mode.clone())),
        (
            "release",
            args.release
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        ),
        (
            "expectedHarnessSha",
            if args.expected_harness_sha.is_empty() {
                Value::Null
            } else {
                Value::String(args.expected_harness_sha.clone())
            },
        ),
        (
            "expectedSourceSha",
            expected_source
                .filter(|value| !value.is_empty())
                .map(|value| Value::String(value.to_string()))
                .unwrap_or(Value::Null),
        ),
        (
            "policy",
            object([
                ("minimumEvidence", Value::String(minimum_evidence)),
                ("requiredTargets", strings(&required_targets)),
                ("requiredJourneys", strings(&required_journeys)),
                (
                    "requiredMatrixProfile",
                    matrix_profile.map(Value::String).unwrap_or(Value::Null),
                ),
                ("requireProtectedArtifacts", Value::Bool(require_protected)),
                ("requireSecretScan", Value::Bool(require_secret_scan)),
            ]),
        ),
        (
            "verdict",
            object([
                ("passed", Value::Bool(errors.is_empty())),
                (
                    "errors",
                    Value::Array(errors.iter().cloned().map(Value::String).collect()),
                ),
            ]),
        ),
        (
            "evidence",
            object([
                (
                    "runIds",
                    Value::Array(
                        runs.iter()
                            .map(|run| Value::String(run.run_id.clone()))
                            .collect(),
                    ),
                ),
                ("builds", Value::Object(builds)),
                (
                    "harnessSha256",
                    if harness_matches {
                        Value::String(args.expected_harness_sha.clone())
                    } else {
                        Value::Null
                    },
                ),
                ("sourceSha256", source_sha),
                ("levels", Value::Object(levels)),
                ("matrix", matrix),
                ("receipt", receipt),
            ]),
        ),
    ]);
    audit_access(
        harness,
        "gate.evaluate",
        if errors.is_empty() {
            "allowed"
        } else {
            "denied"
        },
        Some(&args.app_id),
        Some(&args.mode),
        object([
            (
                "release",
                args.release
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            ),
            (
                "expectedHarnessSha",
                if args.expected_harness_sha.is_empty() {
                    Value::Null
                } else {
                    Value::String(args.expected_harness_sha.clone())
                },
            ),
            (
                "expectedSourceSha",
                expected_source
                    .filter(|value| !value.is_empty())
                    .map(|value| Value::String(value.to_string()))
                    .unwrap_or(Value::Null),
            ),
            ("runs", Value::from(runs.len())),
            ("errors", Value::from(errors.len())),
        ]),
    )?;
    Ok(result)
}

pub fn evaluate(harness: &Path, args: &GateArgs) -> Answer {
    let result = evaluate_value(harness, args)?;
    let passed = truthy(property(&result, "verdict").and_then(|value| property(value, "passed")));
    print_json(&result)?;
    if !passed {
        std::process::exit(1);
    }
    Ok(())
}

pub fn enforce(harness: &Path, args: &GateArgs) -> Answer {
    let status = gate_status_value(harness, &args.app_id)?;
    let enforcement = property(&status, "modes")
        .and_then(|modes| property(modes, &args.mode))
        .and_then(|mode| string_property(mode, "enforcement"));
    let result = if enforcement.as_deref() != Some("required") {
        let result = object([
            ("schemaVersion", Value::from(2)),
            ("appId", Value::String(args.app_id.clone())),
            ("mode", Value::String(args.mode.clone())),
            (
                "verdict",
                object([
                    ("passed", Value::Bool(false)),
                    (
                        "errors",
                        Value::Array(vec![Value::String(
                            "gate is pending green activation".to_string(),
                        )]),
                    ),
                ]),
            ),
            ("status", status),
        ]);
        audit_access(
            harness,
            "gate.enforce",
            "denied",
            Some(&args.app_id),
            Some(&args.mode),
            object([("reason", Value::String("pending-green".to_string()))]),
        )?;
        result
    } else {
        let mut evaluation = evaluate_value(harness, args)?;
        let passed =
            truthy(property(&evaluation, "verdict").and_then(|value| property(value, "passed")));
        let error_count = property(&evaluation, "verdict")
            .and_then(|value| property(value, "errors"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        audit_access(
            harness,
            "gate.enforce",
            if passed { "allowed" } else { "denied" },
            Some(&args.app_id),
            Some(&args.mode),
            object([("errors", Value::from(error_count))]),
        )?;
        evaluation
            .as_object_mut()
            .ok_or_else(|| Failure::config("gate.enforce", "evaluation is not an object"))?
            .insert("status".to_string(), status);
        evaluation
    };
    let passed = truthy(property(&result, "verdict").and_then(|value| property(value, "passed")));
    print_json(&result)?;
    if !passed {
        std::process::exit(1);
    }
    Ok(())
}

pub fn activate(harness: &Path, args: &GateArgs) -> Answer {
    let evaluation = evaluate_value(harness, args)?;
    let passed =
        truthy(property(&evaluation, "verdict").and_then(|value| property(value, "passed")));
    if !passed {
        let errors = value_strings(
            property(&evaluation, "verdict").and_then(|value| property(value, "errors")),
        );
        return Err(Failure::invalid(
            "gate.activate",
            format!("gate activation refused: {}", errors.join("; ")),
        ));
    }
    let app = manifest::load(harness, &args.app_id)?;
    let file = config_file(&app);
    let mut current = if file.exists() {
        serde_json::from_str::<Value>(&fs::read_to_string(&file)?)?
    } else {
        default_config(&args.app_id)
    };
    let activated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let receipt_fingerprint = property(&evaluation, "evidence")
        .and_then(|value| property(value, "receipt"))
        .and_then(|value| string_property(value, "fingerprint"))
        .filter(|value| !value.is_empty())
        .map(Value::String)
        .unwrap_or(Value::Null);
    let activation = object([
        ("enforcement", Value::String("required".to_string())),
        ("activatedAt", Value::String(activated_at)),
        (
            "activationEvidence",
            object([
                (
                    "expectedHarnessSha",
                    property(&evaluation, "expectedHarnessSha")
                        .cloned()
                        .unwrap_or(Value::Null),
                ),
                (
                    "expectedSourceSha",
                    property(&evaluation, "expectedSourceSha")
                        .cloned()
                        .unwrap_or(Value::Null),
                ),
                (
                    "release",
                    property(&evaluation, "release")
                        .cloned()
                        .unwrap_or(Value::Null),
                ),
                (
                    "runIds",
                    property(&evaluation, "evidence")
                        .and_then(|value| property(value, "runIds"))
                        .cloned()
                        .unwrap_or_else(|| Value::Array(Vec::new())),
                ),
                (
                    "builds",
                    property(&evaluation, "evidence")
                        .and_then(|value| property(value, "builds"))
                        .cloned()
                        .unwrap_or_else(|| object([])),
                ),
                (
                    "harnessSha256",
                    property(&evaluation, "evidence")
                        .and_then(|value| property(value, "harnessSha256"))
                        .cloned()
                        .unwrap_or(Value::Null),
                ),
                (
                    "sourceSha256",
                    property(&evaluation, "evidence")
                        .and_then(|value| property(value, "sourceSha256"))
                        .cloned()
                        .unwrap_or(Value::Null),
                ),
                ("receiptFingerprint", receipt_fingerprint),
            ]),
        ),
    ]);
    let current_map = current.as_object_mut().ok_or_else(|| {
        Failure::config(
            "gate.activate",
            format!("{} does not contain a gate object", file.display()),
        )
    })?;
    current_map.insert("schemaVersion".to_string(), Value::from(2));
    let modes = current_map
        .entry("modes")
        .or_insert_with(|| object([]))
        .as_object_mut()
        .ok_or_else(|| Failure::config("gate.activate", "gate modes is not an object"))?;
    modes.insert(args.mode.clone(), activation);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = PathBuf::from(format!(
        "{}.tmp-{}-{}",
        file.to_string_lossy(),
        std::process::id(),
        Utc::now().timestamp_millis()
    ));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    output.write_all(serde_json::to_string_pretty(&current)?.as_bytes())?;
    output.write_all(b"\n")?;
    drop(output);
    fs::rename(&temporary, &file)?;
    audit_access(
        harness,
        "gate.activate",
        "allowed",
        Some(&args.app_id),
        Some(&args.mode),
        object([
            (
                "expectedHarnessSha",
                Value::String(args.expected_harness_sha.clone()),
            ),
            (
                "expectedSourceSha",
                args.expected_source_sha
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            ),
            (
                "release",
                args.release
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            ),
        ]),
    )?;
    print_json(&object([
        ("file", Value::String(file.to_string_lossy().into_owned())),
        ("config", current),
        ("evaluation", evaluation),
    ]))
}

fn random_uuid() -> Result<String, Failure> {
    let mut bytes = [0_u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let encoded = hex::encode(bytes);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &encoded[0..8],
        &encoded[8..12],
        &encoded[12..16],
        &encoded[16..20],
        &encoded[20..32]
    ))
}

fn redact(value: Value, key: &str) -> Value {
    let lower = key.to_ascii_lowercase();
    if [
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
        "token",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return Value::String("[REDACTED]".to_string());
    }
    match value {
        Value::Array(values) => {
            Value::Array(values.into_iter().map(|value| redact(value, "")).collect())
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(name, value)| {
                    let redacted = redact(value, &name);
                    (name, redacted)
                })
                .collect(),
        ),
        other => other,
    }
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn audit_access(
    harness: &Path,
    action: &str,
    outcome: &str,
    app_id: Option<&str>,
    resource: Option<&str>,
    details: Value,
) -> Result<(), Failure> {
    let at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let event_id = random_uuid()?;
    let actor = std::env::var("PROBIERZ_ACTOR")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("GITHUB_ACTOR")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .or_else(|| std::env::var("USER").ok().filter(|value| !value.is_empty()))
        .unwrap_or_else(|| "unknown".to_string());
    let payload = object([
        ("schemaVersion", Value::from(1)),
        ("kind", Value::String("probierz-access-audit".to_string())),
        ("eventId", Value::String(event_id.clone())),
        ("at", Value::String(at.clone())),
        ("actor", Value::String(actor)),
        ("action", Value::String(action.to_string())),
        ("outcome", Value::String(outcome.to_string())),
        (
            "appId",
            app_id
                .map(|value| Value::String(value.to_string()))
                .unwrap_or(Value::Null),
        ),
        ("runId", Value::Null),
        (
            "resource",
            resource
                .map(|value| Value::String(value.to_string()))
                .unwrap_or(Value::Null),
        ),
        (
            "context",
            object([
                ("ci", Value::Bool(nonempty_env("CI").is_some())),
                (
                    "workflow",
                    nonempty_env("GITHUB_WORKFLOW")
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                ),
                (
                    "job",
                    nonempty_env("GITHUB_JOB")
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                ),
            ]),
        ),
        ("details", redact(details, "")),
    ]);
    let hash = hex::encode(Sha256::digest(canonical(&payload).as_bytes()));
    let mut record = payload;
    record
        .as_object_mut()
        .ok_or_else(|| Failure::config("gate.audit", "audit payload is not an object"))?
        .insert("sha256".to_string(), Value::String(hash));
    let directory = harness.join("test-results").join(".audit").join(&at[..10]);
    fs::create_dir_all(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    let timestamp = at.replace([':', '.'], "-");
    let file = directory.join(format!("{timestamp}-{event_id}.json"));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(file)?;
    output.write_all(serde_json::to_string_pretty(&record)?.as_bytes())?;
    output.write_all(b"\n")?;
    Ok(())
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn git_lines(repo: &Path, args: &[&str]) -> Vec<String> {
    let Some(text) = git(repo, args) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn manifest_repositories(app: &manifest::Manifest) -> Vec<&Yaml> {
    yaml_get(&app.document, "repositories")
        .and_then(Yaml::as_sequence)
        .map(|list| list.iter().collect())
        .unwrap_or_default()
}

fn infer_app_id(harness: &Path, repo: &Path) -> Result<Option<String>, Failure> {
    let normalized = normalize_absolute(repo);
    for summary in manifest::list(harness)? {
        let app = manifest::load(harness, &summary.app_id)?;
        if manifest_repositories(&app).iter().any(|repository| {
            yaml_string(yaml_get(repository, "root"))
                .map(|root| normalize_absolute(Path::new(&root)) == normalized)
                .unwrap_or(false)
        }) {
            return Ok(Some(app.app_id));
        }
    }
    Ok(None)
}

fn glob_matches(pattern: &str, text: &str) -> bool {
    fn matches(
        pattern: &[u8],
        text: &[u8],
        pi: usize,
        ti: usize,
        memo: &mut HashMap<(usize, usize), bool>,
    ) -> bool {
        if let Some(result) = memo.get(&(pi, ti)) {
            return *result;
        }
        let result = if pi == pattern.len() {
            ti == text.len()
        } else if pattern[pi] == b'*' && pi + 1 < pattern.len() && pattern[pi + 1] == b'*' {
            matches(pattern, text, pi + 2, ti, memo)
                || (ti < text.len() && matches(pattern, text, pi, ti + 1, memo))
        } else if pattern[pi] == b'*' {
            matches(pattern, text, pi + 1, ti, memo)
                || (ti < text.len() && text[ti] != b'/' && matches(pattern, text, pi, ti + 1, memo))
        } else {
            ti < text.len()
                && pattern[pi] == text[ti]
                && matches(pattern, text, pi + 1, ti + 1, memo)
        };
        memo.insert((pi, ti), result);
        result
    }
    matches(
        pattern.as_bytes(),
        text.as_bytes(),
        0,
        0,
        &mut HashMap::new(),
    )
}

fn affected_journeys(app: &manifest::Manifest, files: &[PathBuf]) -> Vec<String> {
    let mut journeys = BTreeSet::new();
    for repository in manifest_repositories(app) {
        let Some(root) = yaml_string(yaml_get(repository, "root")) else {
            continue;
        };
        let root = normalize_absolute(Path::new(&root));
        let mappings = yaml_get(repository, "mappings")
            .and_then(Yaml::as_sequence)
            .cloned()
            .unwrap_or_default();
        for file in files {
            let file = normalize_absolute(file);
            let Ok(relative) = file.strip_prefix(&root) else {
                continue;
            };
            let relative = relative
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            for mapping in &mappings {
                let patterns = yaml_strings(yaml_get(mapping, "paths"));
                if patterns
                    .iter()
                    .any(|pattern| glob_matches(pattern, &relative))
                {
                    journeys.extend(yaml_strings(yaml_get(mapping, "journeys")));
                }
            }
        }
    }
    journeys.into_iter().collect()
}

fn prepush_value(
    harness: &Path,
    repo: &Path,
    app_id: Option<&str>,
    base: Option<&str>,
    head: Option<&str>,
    run_ci: bool,
    ci_args: &[String],
) -> Result<Value, Failure> {
    let resolved_app = match app_id.map(str::to_string).or(infer_app_id(harness, repo)?) {
        Some(app) => app,
        None => {
            return Ok(object([
                ("ok", Value::Bool(false)),
                (
                    "reason",
                    Value::String(format!(
                        "no probierz app manifest matches {}",
                        repo.display()
                    )),
                ),
            ]))
        }
    };
    let app = manifest::load(harness, &resolved_app)?;
    let resolved_head = head
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| git(repo, &["rev-parse", "HEAD"]));
    let resolved_base =
        if let Some(base) = base.filter(|base| !base.is_empty() && *base != ZERO_SHA) {
            git(repo, &["rev-parse", base])
        } else if let Some(head) = &resolved_head {
            git(repo, &["merge-base", head, "origin/main"])
        } else {
            None
        };
    let Some(resolved_base) = resolved_base else {
        return Ok(object([
            ("ok", Value::Bool(false)),
            ("appId", Value::String(resolved_app)),
            (
                "reason",
                Value::String(
                    "cannot resolve a merge base with origin/main; fetch first or pass --base"
                        .to_string(),
                ),
            ),
        ]));
    };
    let resolved_head = resolved_head.unwrap_or_default();
    let range = format!("{resolved_base}..{resolved_head}");
    let files: Vec<PathBuf> = git_lines(repo, &["diff", "--name-only", &range])
        .into_iter()
        .map(|file| repo.join(file))
        .collect();
    let journeys = affected_journeys(&app, &files);
    if journeys.is_empty() {
        return Ok(object([
            ("ok", Value::Bool(true)),
            ("appId", Value::String(resolved_app)),
            ("base", Value::String(resolved_base)),
            ("head", Value::String(resolved_head)),
            ("affectedJourneys", Value::Array(Vec::new())),
            (
                "verdict",
                object([
                    ("passed", Value::Bool(true)),
                    ("errors", Value::Array(Vec::new())),
                ]),
            ),
            ("note", Value::String("no affected journeys".to_string())),
        ]));
    }
    if run_ci {
        let executable = std::env::current_exe()?;
        let mut command = ProcessCommand::new(executable);
        command
            .arg("--harness")
            .arg(harness)
            .arg("ci")
            .arg(&resolved_base)
            .arg("--app")
            .arg(&resolved_app)
            .args(ci_args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let status = command.status()?;
        if !status.success() {
            return Ok(object([
                ("ok", Value::Bool(false)),
                ("appId", Value::String(resolved_app)),
                ("base", Value::String(resolved_base)),
                ("head", Value::String(resolved_head)),
                ("affectedJourneys", strings(&journeys)),
                (
                    "reason",
                    Value::String(format!(
                        "probierz ci failed (exit {})",
                        status
                            .code()
                            .map(|code| code.to_string())
                            .unwrap_or_else(|| "null".to_string())
                    )),
                ),
            ]));
        }
    }
    let mut history = all_runs(harness, &resolved_app)
        .map_err(|detail| Failure::config("gate.prepush", detail))?;
    history.sort_by(|left, right| {
        js_display(Some(&right.started_at)).cmp(&js_display(Some(&left.started_at)))
    });
    history.truncate(1000);
    let mut run_ids = Vec::new();
    for journey in &journeys {
        if let Some(run) = history
            .iter()
            .find(|run| run.journeys.contains(journey) && run.status == "passed")
        {
            if !run_ids.contains(&run.run_id) {
                run_ids.push(run.run_id.clone());
            }
        }
    }
    if run_ids.is_empty() {
        return Ok(object([
            ("ok", Value::Bool(false)),
            ("appId", Value::String(resolved_app)),
            ("base", Value::String(resolved_base)),
            ("head", Value::String(resolved_head)),
            ("affectedJourneys", strings(&journeys)),
            ("reason", Value::String("no passing runs recorded for the affected journeys; run `probierz ci <base>` (or re-run with --ci) before pushing".to_string())),
        ]));
    }
    let identity = crate::evidence::app_source_identity_value(harness, &resolved_app)?;
    let gate_args = GateArgs {
        app_id: resolved_app.clone(),
        mode: "pull-request".to_string(),
        expected_harness_sha: string_property(
            property(&identity, "harness").unwrap_or(&Value::Null),
            "sha256",
        )
        .unwrap_or_default(),
        expected_source_sha: string_property(
            property(&identity, "app").unwrap_or(&Value::Null),
            "sha256",
        ),
        runs: Some(run_ids.join(",")),
        release: None,
        receipt: None,
        public_key: None,
        fingerprint: None,
    };
    let evaluation = evaluate_value(harness, &gate_args)?;
    let verdict = property(&evaluation, "verdict")
        .cloned()
        .unwrap_or(Value::Null);
    let ok = truthy(property(&verdict, "passed"));
    Ok(object([
        ("ok", Value::Bool(ok)),
        ("appId", Value::String(resolved_app)),
        ("base", Value::String(resolved_base)),
        ("head", Value::String(resolved_head)),
        ("affectedJourneys", strings(&journeys)),
        ("runIds", strings(&run_ids)),
        ("verdict", verdict),
    ]))
}

fn parse_hook_refs(text: &str) -> Option<(String, String)> {
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        if matches!(parts[2], "refs/heads/main" | "refs/heads/master") {
            let base = if parts[3] == ZERO_SHA {
                String::new()
            } else {
                parts[3].to_string()
            };
            return Some((base, parts[1].to_string()));
        }
    }
    None
}

pub fn prepush(harness: &Path, args: &PrepushArgs) -> Answer {
    let repo = args.repo.clone().unwrap_or(std::env::current_dir()?);
    let mut base = args.base.clone();
    let mut head = args.head.clone();
    if args.hook {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        let Some((hook_base, hook_head)) = parse_hook_refs(&input) else {
            println!("prepush-gate: push does not target main; allowed");
            return Ok(());
        };
        base = if hook_base.is_empty() {
            None
        } else {
            Some(hook_base)
        };
        head = Some(hook_head);
    }
    let result = prepush_value(
        harness,
        &repo,
        args.app_id.as_deref(),
        base.as_deref(),
        head.as_deref(),
        args.run_ci,
        &args.ci_args,
    )?;
    if args.hook && !args.json {
        let ok = truthy(property(&result, "ok"));
        let app_id = string_property(&result, "appId").unwrap_or_default();
        let note = string_property(&result, "note")
            .map(|note| format!(" ({note})"))
            .unwrap_or_default();
        println!(
            "prepush-gate {app_id}: {}{note}",
            if ok { "ALLOWED" } else { "BLOCKED" }
        );
        for error in
            value_strings(property(&result, "verdict").and_then(|value| property(value, "errors")))
        {
            println!("  - {error}");
        }
        if let Some(reason) = string_property(&result, "reason") {
            println!("  - {reason}");
        }
    } else {
        print_json(&result)?;
    }
    if !truthy(property(&result, "ok")) {
        std::process::exit(1);
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn present(file: &Path) -> bool {
    fs::symlink_metadata(file).is_ok()
}

fn managed(file: &Path, legacy_command: &str, rust_command: &str) -> bool {
    fs::read_to_string(file)
        .map(|content| {
            content.contains(MANAGED_MARKER)
                || (content.contains(legacy_command) && content.contains("--hook --app"))
                || (content.contains(rust_command)
                    && content.contains("gate-prepush")
                    && content.contains("--hook"))
        })
        .unwrap_or(false)
}

pub fn install(harness: &Path, args: &InstallArgs) -> Answer {
    manifest::load(harness, &args.app_id)?;
    let repo = args.repo.clone().unwrap_or(std::env::current_dir()?);
    let hooks = repo.join(".git").join("hooks");
    if !hooks.exists() {
        return Err(Failure::config(
            "gate.install",
            format!("not a git working tree: {}", repo.display()),
        ));
    }
    let target = hooks.join("pre-push");
    let backup = hooks.join("pre-push.before-probierz-gate");
    let executable = std::env::current_exe()?;
    let rust_command = executable.to_string_lossy().into_owned();
    let legacy_command = harness
        .join("agent")
        .join("prepush-gate.mjs")
        .to_string_lossy()
        .into_owned();
    if present(&backup) && managed(&backup, &legacy_command, &rust_command) {
        fs::remove_file(&backup)?;
    }
    if present(&target) && !present(&backup) && !managed(&target, &legacy_command, &rust_command) {
        fs::rename(&target, &backup)?;
    }
    let script = format!(
        "#!/bin/sh\n{MANAGED_MARKER}\nHOOK_DIR=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nif [ -f \"$HOOK_DIR/pre-push.before-probierz-gate\" ]; then\n  \"$HOOK_DIR/pre-push.before-probierz-gate\" \"$@\" || exit $?\nfi\nGATE_CI=\"--ci\"\nif [ \"${{PROBIERZ_GATE_NO_CI:-}}\" = \"1\" ]; then GATE_CI=\"\"; fi\nexec {} --harness {} gate-prepush --hook --app {} $GATE_CI\n",
        shell_quote(&rust_command),
        shell_quote(&harness.to_string_lossy()),
        shell_quote(&args.app_id),
    );
    fs::create_dir_all(&hooks)?;
    fs::write(&target, script)?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
    print_json(&object([
        (
            "installed",
            Value::String(target.to_string_lossy().into_owned()),
        ),
        ("chained", Value::Bool(backup.exists())),
        ("appId", Value::String(args.app_id.clone())),
    ]))
}
