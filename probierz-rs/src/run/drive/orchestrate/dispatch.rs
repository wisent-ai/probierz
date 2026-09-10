use serde_json::json;
use crate::run::*;

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
