use crate::gate::*;
/// A JSON value as gate messages and comparisons print it; a value the
/// document does not hold is "(not stated)", never JavaScript's "undefined".
pub(crate) fn js_display(value: Option<&Value>) -> String {
    match value {
        None => "(not stated)".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(other) => other.to_string(),
    }
}

pub(crate) fn js_strict_optional_string(actual: Option<&Value>, expected: Option<&str>) -> bool {
    match expected {
        Some(expected) => actual.and_then(Value::as_str) == Some(expected),
        None => actual.is_none(),
    }
}

pub(crate) fn same_set(left: &[String], right: &[String]) -> bool {
    let a: BTreeSet<&String> = left.iter().collect();
    let b: BTreeSet<&String> = right.iter().collect();
    a == b
}

/// Which required cells the runs cover: the cells no run matched and the
/// runs that matched no cell.
pub(crate) struct MatrixCoverage {
    pub(crate) profile: String,
    pub(crate) expected: usize,
    pub(crate) missing: Vec<Value>,
    pub(crate) extra_run_ids: Vec<String>,
}

impl MatrixCoverage {
    /// The coverage as the release receipt records it.
    pub(crate) fn record(&self) -> Value {
        object([
            ("profile", Value::String(self.profile.clone())),
            ("expected", Value::from(self.expected)),
            ("matched", Value::from(self.expected - self.missing.len())),
            ("missing", Value::Array(self.missing.clone())),
            (
                "extraRunIds",
                Value::Array(
                    self.extra_run_ids
                        .iter()
                        .cloned()
                        .map(Value::String)
                        .collect(),
                ),
            ),
        ])
    }
}

pub(crate) fn matrix_coverage(
    app: &manifest::Manifest,
    profile: &str,
    runs: &[Run],
) -> Result<MatrixCoverage, String> {
    // The same expansion a matrix run plans from, so the gate and the run
    // name every cell by one cellId, target and axes.
    let cells = crate::run::planned_cells(app, profile).map_err(|failure| failure.detail)?;
    let mut remaining: Vec<&Run> = runs.iter().collect();
    let mut missing = Vec::new();
    for cell in &cells {
        // Every planned cell states its target and axes.
        let target = string_property(cell, "target")
            .ok_or_else(|| format!("matrix {profile} cell states no target: {cell}"))?;
        let axes = property(cell, "axes")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("matrix {profile} cell states no axes: {cell}"))?;
        let index = remaining.iter().position(|run| {
            run.target == target
                && axes.iter().all(|(name, wanted)| {
                    js_display(property(&run.conditions, name)) == js_display(Some(wanted))
                })
        });
        if let Some(index) = index {
            remaining.remove(index);
        } else {
            missing.push(cell.clone());
        }
    }
    Ok(MatrixCoverage {
        profile: profile.to_string(),
        expected: cells.len(),
        missing,
        extra_run_ids: remaining
            .into_iter()
            .map(|run| run.run_id.clone())
            .collect(),
    })
}

pub(crate) fn receipt_run_value(run: &Run) -> Value {
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
            // A run that wrote no analysis records none: `null`.
            match &run.analysis_path {
                Some(path) => Value::String(path.clone()),
                None => Value::Null,
            },
        ),
    ])
}
