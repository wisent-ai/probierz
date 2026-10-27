use crate::failure::Code;
use crate::run::*;
use serde_json::json;
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
                let status = if result.get("ok").and_then(Value::as_bool) == Some(true) {
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
                env: opts.env.clone(),
                record: opts.record,
                force: opts.force,
                spec: opts.spec.clone(),
                app_id: opts.app_id.clone(),
                kind: Some("pull-request".into()),
            },
        )?;
        if run.get("skipped").and_then(Value::as_bool) == Some(true) {
            // A skipped run names why: the preflight it failed, with what is
            // missing and its remediation, or the resource lock it could
            // not take. One that names neither is refused, never shown as a
            // blocked row with nothing to act on.
            let missing = run["preflight"]["missing"].clone();
            let remediation = match (run.pointer("/preflight/remediation"), run.pointer("/resourceLock/error")) {
                (Some(remediation), _) => remediation.clone(),
                (None, Some(error)) => json!([error]),
                (None, None) => {
                    return Err(Failure::new(
                        "ci",
                        Code::Unknown,
                        format!("the run of {target} was skipped without a preflight or a resource lock naming why: {run}"),
                    ))
                }
            };
            results.push(json!({
                "target": target,
                "runId": run["runId"],
                "status": "blocked",
                "artifactsDir": run["artifactsDir"],
                "manifestPath": run["manifestPath"],
                "missing": missing,
                "remediation": remediation,
                "resourceLock": run["resourceLock"],
            }));
            continue;
        }
        let (analysis, completed) = analyze_and_complete(run, opts.frames)?;
        let passed = completed.get("passed").and_then(Value::as_bool) == Some(true);
        let repair = repair_after_run(harness, &completed, opts.no_repair)?;
        results.push(json!({
            "target": target,
            "runId": completed["runId"],
            "status": if passed { "passed" } else { "failed" },
            "exitCode": completed["exitCode"],
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
        "apps": selection["apps"],
    });
    let mut output = Map::new();
    if files.is_none() {
        // The git selection names the ref it compared against.
        output.insert("ref".into(), selection["ref"].clone());
    }
    output.insert("affected".into(), affected);
    output.insert("results".into(), Value::Array(results));
    output.insert("checks".into(), Value::Array(checks));
    output.insert("summary".into(), summary);
    Ok(Value::Object(output))
}

/// Whether any entry of `list` in the CI `result` ended with `status`.
fn any_ended(result: &Value, list: &str, status: &str) -> bool {
    result
        .get(list)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|entry| entry.get("status").and_then(Value::as_str) == Some(status))
}

/// `probierz ci`: run every affected journey and print the result. A failed
/// journey or check is a refusal and a blocked journey a missing
/// prerequisite, each carrying the whole result, never a process exit, so
/// the MCP server that runs this in process keeps serving.
pub fn ci(harness: &Path, args: &[String]) -> Answer {
    let opts = parse_run_args(args, true)?;
    let files = files_after_flag(args);
    let reference = args
        .first()
        .filter(|arg| !arg.starts_with("--"))
        .map(String::as_str);
    let result = orchestrate(harness, files, reference, &opts)?;
    print_json(&result)?;
    if any_ended(&result, "results", "failed") || any_ended(&result, "checks", "failed") {
        return Err(Failure::new("ci", Code::Refused, result.to_string()));
    }
    if any_ended(&result, "results", "blocked") {
        return Err(Failure::new("ci", Code::Prerequisite, result.to_string()));
    }
    Ok(())
}
