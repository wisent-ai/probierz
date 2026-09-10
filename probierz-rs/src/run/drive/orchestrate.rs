use serde_json::json;
use crate::run::*;
pub(crate) fn orchestrate(
    harness: &Path,
    files: Option<Vec<String>>,
    reference: Option<&str>,
    opts: &RunArgs,
) -> Result<Value, Failure> {
    let selection = if let Some(files) = &files {
        affected_targets(harness, files)?
    } else {
        affected_from_git(harness, reference)?
    };
    let mut app_ids: BTreeSet<String> = selection
        .get("apps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|app| app.get("appId").and_then(Value::as_str).map(str::to_string))
        .collect();
    if let Some(app_id) = &opts.app_id {
        app_ids.insert(app_id.clone());
    }
    let mut checks = Vec::new();
    for app_id in app_ids {
        let (status, result) = match crate::authoring::validate_accessibility(harness, &app_id) {
            Ok(result) => {
                let status = if result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    "passed"
                } else {
                    "failed"
                };
                (status, result)
            }
            Err(error) => ("failed", json!({ "ok": false, "error": error.to_string() })),
        };
        checks.push(json!({
            "name": format!("accessibility:{app_id}"),
            "status": status,
            "result": result,
        }));
    }

    let mut results = Vec::new();
    for target in selection
        .get("targets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let run = run_surface(
            harness,
            target,
            RunOptions {
                local: false,
                host_selector: byk_host_selector(None, &BTreeMap::new()),
                env: opts.env.clone(),
                record: opts.record,
                timeout_ms: opts.timeout_ms,
                force: opts.force,
                spec: opts.spec.clone(),
                app_id: opts.app_id.clone(),
                kind: Some("pull-request".into()),
                resource_wait_ms: Some(opts.resource_wait_ms.unwrap_or(10 * 60 * 1000)),
            },
        )?;
        if run.get("skipped").and_then(Value::as_bool) == Some(true) {
            let remediation = run
                .pointer("/preflight/remediation")
                .cloned()
                .unwrap_or_else(|| {
                    run.pointer("/resourceLock/error")
                        .map(|value| json!([value]))
                        .unwrap_or_else(|| json!([]))
                });
            results.push(json!({
                "target": target,
                "runId": run["runId"],
                "status": "blocked",
                "artifactsDir": run["artifactsDir"],
                "manifestPath": run["manifestPath"],
                "missing": run.pointer("/preflight/missing").cloned().unwrap_or_else(|| json!([])),
                "remediation": remediation,
                "resourceLock": run.get("resourceLock").cloned().unwrap_or(Value::Null),
            }));
            continue;
        }
        let analyzed = analyze_run(
            Path::new(run.get("reportPath").and_then(Value::as_str).unwrap_or("")),
            Some(Path::new(
                run.get("artifactsDir")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )),
            run.get("tool").and_then(Value::as_str),
            opts.frames,
            run.get("runId").and_then(Value::as_str),
        );
        let (analysis, completed) = match analyzed {
            Ok(analysis) => {
                let complete = complete_run(run, Some(&analysis), None)?;
                (analysis, complete)
            }
            Err(error) => {
                let analysis = json!({ "error": error.detail });
                let complete = complete_run(run, None, Some(&error.detail))?;
                (analysis, complete)
            }
        };
        let passed = completed
            .get("passed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let repair =
            if !passed && !opts.no_repair && std::env::var_os("PROBIERZ_REPAIR_SUPPRESS").is_none()
            {
                crate::authoring::repair_failed_run(
                    harness,
                    completed
                        .get("appId")
                        .and_then(Value::as_str)
                        .unwrap_or("probierz"),
                    completed.get("runId").and_then(Value::as_str),
                    1,
                    false,
                )?
            } else {
                Value::Null
            };
        results.push(json!({
            "target": target,
            "runId": completed["runId"],
            "status": if passed { "passed" } else { "failed" },
            "exitCode": completed["exitCode"],
            "timedOut": completed["timedOut"],
            "durationMs": completed["durationMs"],
            "reportPath": completed["reportPath"],
            "artifactsDir": completed["artifactsDir"],
            "manifestPath": completed["manifestPath"],
            "analysisPath": completed["analysisPath"],
            "evidence": completed["evidence"],
            "analysis": analysis,
            "repair": repair,
        }));
    }
    let count = |status: &str| {
        results
            .iter()
            .filter(|result| result.get("status").and_then(Value::as_str) == Some(status))
            .count()
    };
    let check_count = |status: &str| {
        checks
            .iter()
            .filter(|check| check.get("status").and_then(Value::as_str) == Some(status))
            .count()
    };
    let summary = json!({
        "total": results.len() + checks.len(),
        "passed": count("passed") + check_count("passed"),
        "failed": count("failed") + check_count("failed"),
        "blocked": count("blocked"),
        "ran": count("passed") + count("failed"),
        "checks": checks.len(),
    });
    let affected = json!({
        "targets": selection["targets"],
        "crossCutting": selection["crossCutting"],
        "files": selection["files"],
        "apps": selection.get("apps").cloned().unwrap_or_else(|| json!([])),
    });
    let mut output = Map::new();
    if files.is_none() {
        output.insert(
            "ref".into(),
            selection
                .get("ref")
                .cloned()
                .or_else(|| reference.map(|value| json!(value)))
                .unwrap_or_else(|| json!("HEAD")),
        );
    }
    output.insert("affected".into(), affected);
    output.insert("results".into(), Value::Array(results));
    output.insert("checks".into(), Value::Array(checks));
    output.insert("summary".into(), summary);
    Ok(Value::Object(output))
}

pub fn ci(harness: &Path, args: &[String]) -> Answer {
    let opts = parse_run_args(args, true)?;
    let files = files_after_flag(args);
    let reference = args
        .first()
        .filter(|arg| !arg.starts_with("--"))
        .map(String::as_str);
    let result = orchestrate(harness, files, reference, &opts)?;
    let failed = result
        .pointer("/summary/failed")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let blocked = result
        .pointer("/summary/blocked")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    print_json(&result)?;
    if failed > 0 {
        std::process::exit(1);
    }
    if blocked > 0 {
        std::process::exit(3);
    }
    Ok(())
}

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

pub fn matrix(harness: &Path, app_id: &str, profile: &str, args: &[String]) -> Answer {
    let plan_only = args.iter().any(|arg| arg == "--plan");
    let release_at = args.iter().position(|arg| arg == "--release");
    let release = if let Some(index) = release_at {
        Some(value_after(args, index, "--release")?)
    } else {
        None
    };
    if profile == "release" && !plan_only && release.is_none() {
        return Err(fail(
            "cli.matrix",
            "release matrix execution needs --release <id>",
        ));
    }
    let mut env = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--plan" {
            index += 1;
            continue;
        }
        if args[index] == "--release" {
            index += 2;
            continue;
        }
        let Some((name, value)) = args[index].split_once('=') else {
            return Err(fail(
                "cli.matrix",
                format!("unexpected matrix argument: {}", args[index]),
            ));
        };
        env.insert(name.into(), value.into());
        index += 1;
    }

    let plan = plan_matrix(harness, app_id, profile)?;
    if plan_only {
        return print_json(&plan);
    }
    let effective = release.or_else(|| {
        plan.get("release")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    if profile == "release" && effective.is_none() {
        return Err(fail(
            "run.matrix",
            "release matrix execution needs a release ID",
        ));
    }
    let artifact_key = env
        .get("PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE")
        .cloned()
        .or_else(|| std::env::var("PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE").ok());
    if plan.get("artifactEncryption").and_then(Value::as_str) == Some("required")
        && artifact_key.is_none()
    {
        return Err(fail(
            "run.matrix",
            "matrix requires PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE",
        ));
    }
    env.remove("PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE");
    for cell in plan
        .get("cells")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for (name, value) in &env {
            if let Some(axis) = cell
                .get("axes")
                .and_then(Value::as_object)
                .and_then(|axes| axes.get(name))
                .and_then(Value::as_str)
            {
                if axis != value {
                    return Err(fail(
                        "run.matrix",
                        format!("matrix axis {name} cannot be overridden"),
                    ));
                }
            }
        }
    }

    let protect = |run: &Value| -> (Value, Value) {
        let Some(key) = artifact_key.as_deref() else {
            return (Value::Null, Value::Null);
        };
        let result = crate::evidence::protect_run(
            harness,
            app_id,
            run.get("runId").and_then(Value::as_str).unwrap_or_default(),
            Some(profile),
            Some(Path::new(key)),
            plan.get("removePlaintextAfterProtection")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        );
        match result {
            Ok(artifact) => (artifact, Value::Null),
            Err(error) => (Value::Null, Value::String(error.to_string())),
        }
    };

    let mut results = Vec::new();
    for cell in plan
        .get("cells")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mut cell_env: BTreeMap<String, String> = cell
            .get("env")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_string())))
            .collect();
        cell_env.extend(env.clone());
        if let Some(release) = &effective {
            cell_env.insert("PROBIERZ_RELEASE".into(), release.clone());
        }
        let run = run_surface(
            harness,
            cell.get("target").and_then(Value::as_str).unwrap_or(""),
            RunOptions {
                local: false,
                host_selector: byk_host_selector(None, &BTreeMap::new()),
                env: cell_env,
                record: plan.get("record").and_then(Value::as_bool).unwrap_or(true),
                timeout_ms: plan.get("timeoutMs").and_then(Value::as_u64).unwrap_or(0),
                force: false,
                spec: cell.get("spec").and_then(Value::as_str).map(str::to_string),
                app_id: Some(app_id.into()),
                kind: Some(profile.into()),
                resource_wait_ms: plan.get("resourceWaitMs").and_then(Value::as_u64),
            },
        )?;
        let mut public = cell.clone();
        if let Some(map) = public.get_mut("env").and_then(Value::as_object_mut) {
            for (name, value) in map.iter_mut() {
                if sensitive_key(name) {
                    *value = json!("[REDACTED]");
                }
            }
        }
        if run.get("skipped").and_then(Value::as_bool) == Some(true)
            || run.get("canceled").and_then(Value::as_bool) == Some(true)
        {
            let canceled = run
                .get("canceled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let (artifact, protection_error) = protect(&run);
            public.as_object_mut().expect("object").extend(
                json!({
                    "status": if canceled { "canceled" } else { "blocked" },
                    "run": run,
                    "protectedArtifact": artifact,
                    "protectionError": protection_error,
                })
                .as_object()
                .expect("object")
                .clone(),
            );
            results.push(public);
            continue;
        }

        let analyzed = analyze_run(
            Path::new(run.get("reportPath").and_then(Value::as_str).unwrap_or("")),
            Some(Path::new(
                run.get("artifactsDir")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )),
            run.get("tool").and_then(Value::as_str),
            plan.get("frames").and_then(Value::as_f64).unwrap_or(0.0),
            run.get("runId").and_then(Value::as_str),
        );
        let (analysis, completed) = match analyzed {
            Ok(analysis) => {
                let complete = complete_run(run, Some(&analysis), None)?;
                (analysis, complete)
            }
            Err(error) => {
                let analysis = json!({ "error": error.detail });
                let complete = complete_run(run, None, Some(&error.detail))?;
                (analysis, complete)
            }
        };
        let (artifact, protection_error) = protect(&completed);
        let passed = completed
            .get("passed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && protection_error.is_null();
        let level = if !completed
            .get("passed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "E0"
        } else if completed
            .pointer("/conditions/record")
            .and_then(Value::as_bool)
            == Some(true)
            && completed
                .pointer("/evidence/report")
                .and_then(Value::as_bool)
                == Some(true)
            && completed
                .pointer("/evidence/analysis")
                .and_then(Value::as_bool)
                == Some(true)
            && completed
                .pointer("/evidence/capturePresent")
                .and_then(Value::as_bool)
                == Some(true)
        {
            "E3"
        } else {
            "E2"
        };
        public.as_object_mut().expect("object").extend(
            json!({
                "status": if passed { "passed" } else { "failed" },
                "evidenceLevel": level,
                "run": completed,
                "analysis": analysis,
                "protectedArtifact": artifact,
                "protectionError": protection_error,
            })
            .as_object()
            .expect("object")
            .clone(),
        );
        results.push(public);
    }

    let count = |status: &str| {
        results
            .iter()
            .filter(|result| result.get("status").and_then(Value::as_str) == Some(status))
            .count()
    };
    let required = plan
        .get("minimumCellEvidence")
        .and_then(Value::as_str)
        .unwrap_or("E3");
    let rank = |level: &str| match level {
        "E0" => 0,
        "E1" => 1,
        "E2" => 2,
        "E3" => 3,
        _ => -1,
    };
    let evidence_satisfied = results.iter().all(|result| {
        rank(
            result
                .get("evidenceLevel")
                .and_then(Value::as_str)
                .unwrap_or(""),
        ) >= rank(required)
    });
    let passed = count("passed") == results.len() && evidence_satisfied;
    let output = json!({
        "schemaVersion": 1,
        "appId": app_id,
        "profile": profile,
        "release": effective,
        "generatedAt": now_iso(),
        "verdict": {
            "passed": passed,
            "evidenceLevel": if passed { json!("E4") } else { Value::Null },
            "minimumCellEvidence": required,
            "evidenceSatisfied": evidence_satisfied,
        },
        "summary": {
            "total": results.len(),
            "passed": count("passed"),
            "failed": count("failed"),
            "blocked": count("blocked"),
            "canceled": count("canceled"),
        },
        "results": results,
    });
    print_json(&output)?;
    if !passed {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glob_double_star_crosses_directories_but_star_does_not() {
        assert!(glob_matches("src/**/view.ts", "src/a/b/view.ts"));
        assert!(!glob_matches("src/*/view.ts", "src/a/b/view.ts"));
    }
}
