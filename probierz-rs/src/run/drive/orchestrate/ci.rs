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

