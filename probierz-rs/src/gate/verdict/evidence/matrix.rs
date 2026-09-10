use crate::gate::*;
pub(crate) fn canonical(value: &Value) -> String {
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

pub(crate) fn js_display(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
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

pub(crate) fn yaml_mapping<'a>(value: Option<&'a Yaml>) -> Option<&'a serde_yaml::Mapping> {
    value.and_then(Yaml::as_mapping)
}

pub(crate) fn matrix_cells(app: &manifest::Manifest, profile: &str) -> Result<Vec<Value>, String> {
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

pub(crate) fn matrix_coverage(app: &manifest::Manifest, profile: &str, runs: &[Run]) -> Result<Value, String> {
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
            run.analysis_path
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        ),
    ])
}

