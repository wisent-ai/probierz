use serde_json::json;
use crate::run::*;

pub(crate) fn matrix_expand(dimensions: Option<&serde_yaml::Value>) -> Vec<BTreeMap<String, String>> {
    let mut cells = vec![BTreeMap::new()];
    let mut values: Vec<(&str, &serde_yaml::Value)> = dimensions
        .and_then(serde_yaml::Value::as_mapping)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| Some((key.as_str()?, value)))
        .collect();
    values.sort_by_key(|(name, _)| *name);
    for (name, items) in values {
        let mut expanded = Vec::new();
        for cell in &cells {
            for value in items.as_sequence().into_iter().flatten() {
                let mut next = cell.clone();
                next.insert(name.into(), yaml_string(value).unwrap_or_default());
                expanded.push(next);
            }
        }
        cells = expanded;
    }
    cells
}
pub(crate) fn cell_id(target: &str, env: &BTreeMap<String, String>) -> String {
    let stable = json!({"env":env,"target":target});
    hex::encode(Sha256::digest(stable.to_string().as_bytes()))[..16].into()
}

pub(crate) fn plan_matrix(harness: &Path, app_id: &str, profile: &str) -> Result<Value, Failure> {
    let declaration = manifest::load(harness, app_id)?;
    let policy = declaration
        .document
        .get("matrix")
        .and_then(|matrix| matrix.get(profile))
        .ok_or_else(|| {
            fail(
                "run.matrix",
                format!("app {app_id} has no {profile} matrix"),
            )
        })?;
    let surfaces = declaration
        .document
        .get("surfaces")
        .and_then(serde_yaml::Value::as_mapping)
        .ok_or_else(|| Failure::config("run.matrix", "surfaces missing"))?;
    let mut targets: Vec<String> = policy
        .get("targets")
        .and_then(serde_yaml::Value::as_sequence)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_yaml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| {
            surfaces
                .keys()
                .filter_map(serde_yaml::Value::as_str)
                .map(str::to_string)
                .collect()
        });
    targets.sort();
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
        let mut dimensions = policy
            .get("dimensions")
            .and_then(serde_yaml::Value::as_mapping)
            .cloned()
            .unwrap_or_default();
        if let Some(specific) = policy
            .get("surfaces")
            .and_then(|surfaces| surfaces.get(&name))
            .and_then(|surface| surface.get("dimensions"))
            .and_then(serde_yaml::Value::as_mapping)
        {
            dimensions.extend(specific.clone());
        }
        for axes in matrix_expand(Some(&serde_yaml::Value::Mapping(dimensions))) {
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
    let max = policy
        .get("maxCells")
        .and_then(serde_yaml::Value::as_u64)
        .unwrap_or(128) as usize;
    if cells.len() > max {
        return Err(fail(
            "run.matrix",
            format!(
                "matrix {profile} expands to {} cells (max {max})",
                cells.len()
            ),
        ));
    }
    let frames = policy
        .get("frames")
        .and_then(serde_yaml::Value::as_f64)
        .or_else(|| {
            policy
                .get("frames")
                .and_then(serde_yaml::Value::as_u64)
                .map(|value| value as f64)
        })
        .unwrap_or(0.0);
    Ok(json!({
        "schemaVersion": 1,
        "appId": app_id,
        "owner": declaration.document.get("owner").and_then(serde_yaml::Value::as_str).unwrap_or(""),
        "profile": profile,
        "record": policy.get("record").and_then(serde_yaml::Value::as_bool) != Some(false),
        "frames": number(frames),
        "timeoutMs": policy.get("timeoutMs").and_then(serde_yaml::Value::as_u64).unwrap_or(0),
        "resourceWaitMs": policy.get("resourceWaitMs").and_then(serde_yaml::Value::as_u64).unwrap_or(10 * 60 * 1000),
        "maximumParallel": policy.get("maximumParallel").and_then(serde_yaml::Value::as_u64).unwrap_or(4).max(1),
        "minimumCellEvidence": policy.get("minimumCellEvidence").and_then(serde_yaml::Value::as_str).unwrap_or("E3"),
        "artifactEncryption": policy.get("artifactEncryption").and_then(serde_yaml::Value::as_str).unwrap_or("optional"),
        "removePlaintextAfterProtection": policy.get("removePlaintextAfterProtection").and_then(serde_yaml::Value::as_bool).unwrap_or(false),
        "release": policy.get("release").and_then(serde_yaml::Value::as_str),
        "cells": cells,
    }))
}

