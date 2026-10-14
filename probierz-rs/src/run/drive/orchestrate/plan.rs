use crate::run::*;
use serde_json::json;

/// Every combination of the dimensions' values. A dimension that is not a
/// list, or a value that is not a scalar, is refused naming it: read as no
/// values the target expanded to no cells and left the matrix without it, and
/// read as the empty value the cell ran under a condition nobody declared.
pub(crate) fn matrix_expand(
    dimensions: Option<&serde_yaml::Value>,
) -> Result<Vec<BTreeMap<String, String>>, String> {
    let mut cells = vec![BTreeMap::new()];
    let mut values: Vec<(&str, &serde_yaml::Value)> = dimensions
        .and_then(serde_yaml::Value::as_mapping)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| Some((key.as_str()?, value)))
        .collect();
    values.sort_by_key(|(name, _)| *name);
    for (name, items) in values {
        let items = items
            .as_sequence()
            .ok_or_else(|| format!("dimension {name} is not a list of values"))?;
        let items = items
            .iter()
            .map(|value| {
                yaml_string(value)
                    .ok_or_else(|| format!("dimension {name} value {value:?} is not a scalar"))
            })
            .collect::<Result<Vec<String>, String>>()?;
        let mut expanded = Vec::new();
        for cell in &cells {
            for value in &items {
                let mut next = cell.clone();
                next.insert(name.into(), value.clone());
                expanded.push(next);
            }
        }
        cells = expanded;
    }
    Ok(cells)
}
pub(crate) fn cell_id(target: &str, env: &BTreeMap<String, String>) -> String {
    let stable = json!({"env":env,"target":target});
    hex::encode(Sha256::digest(stable.to_string().as_bytes()))[..16].into()
}

/// The `matrix.<profile>` policy a product declares.
pub(crate) fn matrix_policy<'a>(
    declaration: &'a manifest::Manifest,
    profile: &str,
) -> Result<&'a serde_yaml::Value, Failure> {
    declaration
        .document
        .get("matrix")
        .and_then(|matrix| matrix.get(profile))
        .ok_or_else(|| {
            fail(
                "run.matrix",
                format!("app {} has no {profile} matrix", declaration.app_id),
            )
        })
}

/// The names a matrix field lists. A field that is not a list of names is
/// refused naming it.
fn listed_names(field: &serde_yaml::Value, what: &str) -> Result<Vec<String>, Failure> {
    field
        .as_sequence()
        .ok_or_else(|| fail("run.matrix", format!("{what} is not a list")))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| fail("run.matrix", format!("{what} holds {value:?}, not a name")))
        })
        .collect()
}

/// The cells one matrix profile expands to: the one expansion a matrix run
/// plans and the release gate requires, so both name each cell by the same
/// cellId, target and axes. A profile without `targets` covers every
/// surface; one without `dimensions` has a single cell per target.
pub(crate) fn planned_cells(
    declaration: &manifest::Manifest,
    profile: &str,
) -> Result<Vec<Value>, Failure> {
    let policy = matrix_policy(declaration, profile)?;
    let surfaces = declaration
        .document
        .get("surfaces")
        .and_then(serde_yaml::Value::as_mapping)
        .ok_or_else(|| Failure::config("run.matrix", "surfaces missing"))?;
    let mut targets: Vec<String> = match policy.get("targets") {
        Some(listed) => listed_names(listed, &format!("matrix {profile} targets"))?,
        None => surfaces
            .keys()
            .filter_map(serde_yaml::Value::as_str)
            .map(str::to_string)
            .collect(),
    };
    targets.sort();
    let mapping = |field: Option<&serde_yaml::Value>, what: String| match field {
        None => Ok(serde_yaml::Mapping::new()),
        Some(field) => field
            .as_mapping()
            .cloned()
            .ok_or_else(|| fail("run.matrix", format!("{what} is not a mapping"))),
    };
    let mut cells = Vec::new();
    for name in targets {
        let surface = surfaces
            .get(&serde_yaml::Value::String(name.clone()))
            .ok_or_else(|| {
                fail(
                    "run.matrix",
                    format!("matrix {profile} references unknown target: {name}"),
                )
            })?;
        let mut dimensions = mapping(
            policy.get("dimensions"),
            format!("matrix {profile} dimensions"),
        )?;
        dimensions.extend(mapping(
            policy
                .get("surfaces")
                .and_then(|surfaces| surfaces.get(&name))
                .and_then(|surface| surface.get("dimensions")),
            format!("matrix {profile} surface {name} dimensions"),
        )?);
        let expanded =
            matrix_expand(Some(&serde_yaml::Value::Mapping(dimensions))).map_err(|detail| {
                fail(
                    "run.matrix",
                    format!("matrix {profile} for {name}: {detail}"),
                )
            })?;
        for axes in expanded {
            let mut public_env = yaml_ordered_strings(surface.get("conditions"));
            for (name, value) in &axes {
                public_env.insert(name.clone(), Value::String(value.clone()));
            }
            let env: BTreeMap<String, String> = public_env
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_string())))
                .collect();
            let index = cells.len();
            let spec = policy
                .get("surfaces")
                .and_then(|surfaces| surfaces.get(&name))
                .and_then(|surface| surface.get("spec"))
                .and_then(serde_yaml::Value::as_str)
                .or_else(|| surface.get("spec").and_then(serde_yaml::Value::as_str));
            let mut journeys = manifest::surface_journeys(surface, &env);
            journeys.sort();
            cells.push(json!({
                "index": index,
                "cellId": cell_id(&name, &env),
                "target": name,
                "spec": spec,
                "journeys": journeys,
                "axes": axes,
                "env": Value::Object(public_env),
            }));
        }
    }
    if cells.is_empty() {
        return Err(fail(
            "run.matrix",
            format!("matrix {profile} expands to no cells; declare targets and non-empty dimension values"),
        ));
    }
    // A profile bounds its matrix only when it states maxCells.
    if let Some(max) = policy.get("maxCells").and_then(serde_yaml::Value::as_u64) {
        if cells.len() as u64 > max {
            return Err(fail(
                "run.matrix",
                format!(
                    "matrix {profile} expands to {} cells (max {max})",
                    cells.len()
                ),
            ));
        }
    }
    Ok(cells)
}

pub(crate) fn plan_matrix(harness: &Path, app_id: &str, profile: &str) -> Result<Value, Failure> {
    let declaration = manifest::load(harness, app_id)?;
    let policy = matrix_policy(&declaration, profile)?;
    let cells = planned_cells(&declaration, profile)?;
    // What the profile declares, and only that: an undeclared frame count or
    // protection policy is null in the plan, never a made-up 0, "optional"
    // or false.
    let frames = policy
        .get("frames")
        .and_then(serde_yaml::Value::as_f64)
        .or_else(|| {
            policy
                .get("frames")
                .and_then(serde_yaml::Value::as_u64)
                .map(|value| value as f64)
        });
    Ok(json!({
        "schemaVersion": 1,
        "appId": app_id,
        "owner": declaration.owner()?,
        "profile": profile,
        "record": policy.get("record").and_then(serde_yaml::Value::as_bool) != Some(false),
        "frames": frames.map(number),
        "maximumParallel": policy.get("maximumParallel").and_then(serde_yaml::Value::as_u64),
        "minimumCellEvidence": policy.get("minimumCellEvidence").and_then(serde_yaml::Value::as_str),
        "artifactEncryption": policy.get("artifactEncryption").and_then(serde_yaml::Value::as_str),
        "removePlaintextAfterProtection": policy.get("removePlaintextAfterProtection").and_then(serde_yaml::Value::as_bool),
        "release": policy.get("release").and_then(serde_yaml::Value::as_str),
        "cells": cells,
    }))
}
