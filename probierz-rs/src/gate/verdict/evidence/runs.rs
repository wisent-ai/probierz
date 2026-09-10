use crate::gate::*;

#[derive(Debug, Clone)]
pub(crate) struct Run {
    pub(crate) manifest_path: PathBuf,
    pub(crate) run_id: String,
    pub(crate) kind: String,
    pub(crate) target: String,
    pub(crate) spec: Value,
    pub(crate) status: String,
    pub(crate) started_at: Value,
    pub(crate) completed_at: Value,
    pub(crate) harness: Value,
    pub(crate) source: Value,
    pub(crate) build: Value,
    pub(crate) journeys: Vec<String>,
    pub(crate) device: Value,
    pub(crate) conditions: Value,
    pub(crate) evidence: Value,
    pub(crate) artifacts: Vec<Value>,
    pub(crate) protection: Value,
    pub(crate) analysis_path: Option<String>,
}

pub(crate) fn normalized_status(document: &Value) -> String {
    match string_property(document, "status").as_deref() {
        Some("passed") | Some("executed") => "passed".to_string(),
        Some("blocked") => "blocked".to_string(),
        Some("canceled") => "canceled".to_string(),
        Some("failed") => "failed".to_string(),
        _ if truthy(property(document, "completedAt")) => "failed".to_string(),
        _ => "incomplete".to_string(),
    }
}

pub(crate) fn run_from_file(file: &Path) -> Result<Option<Run>, String> {
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

pub(crate) fn manifests_below(root: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
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

pub(crate) fn all_runs(harness: &Path, app_id: &str) -> Result<Vec<Run>, String> {
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

pub(crate) fn get_run(harness: &Path, app_id: &str, run_id: &str) -> Result<Run, String> {
    all_runs(harness, app_id)?
        .into_iter()
        .find(|run| run.run_id == run_id)
        .ok_or_else(|| format!("run not found for {app_id}: {run_id}"))
}

pub(crate) fn evidence_level(run: &Run) -> &'static str {
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

pub(crate) fn sha256_file(file: &Path) -> Result<String, String> {
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

pub(crate) fn normalize_absolute(path: &Path) -> PathBuf {
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

