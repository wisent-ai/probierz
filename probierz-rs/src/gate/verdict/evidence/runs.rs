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

pub(crate) fn run_from_file(file: &Path) -> Result<Option<Run>, String> {
    // Only a missing manifest is no run; one that cannot be read or parsed
    // is evidence the gate could not weigh, said with its cause.
    let Some(document) =
        crate::json_file::read(file).map_err(|error| format!("run manifest {}", error.detail))?
    else {
        return Ok(None);
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
    // A run is weighed by its id and target; a manifest stating neither is
    // refused, not weighed as run '' on target ''.
    let stated = |field: &str| {
        string_property(&document, field)
            .ok_or_else(|| format!("run manifest {} states no {field}", file.display()))
    };
    let run_id = stated("runId")?;
    let target = stated("target")?;
    Ok(Some(Run {
        manifest_path: directory.join("run-manifest.json"),
        run_id,
        kind: string_property(&document, "kind").unwrap_or_else(|| "adhoc".to_string()),
        target,
        spec: property(&document, "spec").cloned().unwrap_or(Value::Null),
        status: crate::evidence::normalized_status(&document),
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

pub(crate) fn all_runs(harness: &Path, app_id: &str) -> Result<Vec<Run>, String> {
    let files = crate::evidence::manifests_below(&harness.join("test-results").join(app_id))
        .map_err(|failure| failure.to_string())?;
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

#[cfg(test)]
#[path = "../../../../../tests/gate/run-evidence.rs"]
mod tests;
