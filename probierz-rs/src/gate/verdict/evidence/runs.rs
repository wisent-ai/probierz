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
    // Manifests are found below the harness's results, so each lies in its
    // run's directory.
    let directory = file
        .parent()
        .ok_or_else(|| format!("run manifest {} lies in no directory", file.display()))?;
    let analysis_path = string_property(&document, "analysisPath");
    // The journeys the app manifest declared for the run; a run without an
    // app manifest declared none. A journey that is not a name is refused.
    let journeys =
        match property(&document, "appManifest").and_then(|value| property(value, "journeys")) {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(list)) => list
                .iter()
                .map(|journey| {
                    journey.as_str().map(str::to_string).ok_or_else(|| {
                        format!(
                            "run manifest {} declares a journey that is not a name: {journey}",
                            file.display()
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            Some(other) => {
                return Err(format!(
                    "run manifest {} declares journeys that are not a list: {other}",
                    file.display()
                ))
            }
        };
    // A run is weighed by its id, target and kind, which every run writes; a
    // manifest stating none of them is refused, not weighed as run '' on
    // target '' or as an ad-hoc run.
    let stated = |field: &str| {
        string_property(&document, field)
            .ok_or_else(|| format!("run manifest {} states no {field}", file.display()))
    };
    let run_id = stated("runId")?;
    let target = stated("target")?;
    let kind = stated("kind")?;
    // A record the run did not write is `null`; conditions it stated none of
    // are an empty set.
    let recorded = |name: &str| match property(&document, name) {
        Some(value) => value.clone(),
        None => Value::Null,
    };
    let conditions = match property(&document, "conditions") {
        Some(conditions) => conditions.clone(),
        None => object([]),
    };
    Ok(Some(Run {
        manifest_path: directory.join("run-manifest.json"),
        run_id,
        kind,
        target,
        spec: recorded("spec"),
        status: crate::evidence::normalized_status(&document),
        started_at: recorded("startedAt"),
        completed_at: recorded("completedAt"),
        harness: recorded("harness"),
        source: recorded("source"),
        build: recorded("build"),
        journeys,
        device: recorded("device"),
        conditions,
        evidence: recorded("evidence"),
        artifacts: value_array(
            property(&document, "artifacts"),
            &format!("run manifest {} artifacts", file.display()),
        )?
        .to_vec(),
        protection: recorded("protection"),
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
    level_of(
        run.status == "passed",
        Some(&run.conditions),
        Some(&run.evidence),
    )
}

/// `path` made absolute against the working directory, with `.` and `..`
/// resolved by name. A working directory that cannot be read is refused,
/// never taken as `.`, which compared a relative path with itself.
pub(crate) fn normalize_absolute(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                format!(
                    "the working directory cannot be read to resolve {}: {error}",
                    path.display()
                )
            })?
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
    Ok(normalized)
}

#[cfg(test)]
#[path = "../../../../../tests/gate/run-evidence.rs"]
mod tests;
