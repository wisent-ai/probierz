use crate::run::*;
use serde_json::json;
pub(crate) fn complete_run(
    mut run: Value,
    analysis: Option<&Value>,
    analysis_error: Option<&str>,
) -> Result<Value, Failure> {
    if run.get("canceled").and_then(Value::as_bool) == Some(true) {
        return Ok(run);
    }
    // Every run records where its artifacts, manifest and id are; a run
    // value without one is refused, never completed under an empty path.
    let recorded = |field: &str| {
        run.get(field)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| fail("run.complete", format!("the run records no {field}: {run}")))
    };
    let artifacts = PathBuf::from(recorded("artifactsDir")?);
    let manifest_path = PathBuf::from(recorded("manifestPath")?);
    let run_id = recorded("runId")?;
    let analysis_path = artifacts.join("analysis.json");
    let payload = match (analysis_error, analysis) {
        (Some(error), _) => json!({"runId":run_id,"error":error}),
        (None, Some(Value::Object(fields))) => {
            let mut fields = fields.clone();
            fields.insert("runId".into(), json!(run_id));
            Value::Object(fields)
        }
        (None, other) => {
            return Err(fail(
                "run.complete",
                format!(
                "run {run_id} completes with neither an analysis object nor its error: {other:?}"
            ),
            ))
        }
    };
    write_json(&analysis_path, &payload)?;
    // What the analysis lists under `pointer`; an analysis that lists
    // nothing there (or no analysis, when it failed) lists none.
    let listed = |pointer: &str| match analysis.and_then(|value| value.pointer(pointer)) {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    let capture_errors = listed("/captureErrors");
    let media = listed("/media");
    let missing: Vec<Value> = media
        .iter()
        .filter(|item| item.get("missing").and_then(Value::as_bool) == Some(true))
        .cloned()
        .collect();
    let crashes = listed("/diagnostics/crashes");
    // The analysis's executed and failed check counts as it states them; a
    // count it does not state is never read as zero checks or zero failures.
    let checks = |field: &str| {
        analysis
            .and_then(|value| value.get(field))
            .and_then(Value::as_u64)
    };
    let (total, failed) = (checks("total"), checks("failed"));
    let failed_any = failed.and_then(std::num::NonZeroU64::new);
    let valid = analysis_error.is_none()
        && analysis.is_some_and(|value| {
            value.get("runId").and_then(Value::as_str) == Some(&run_id)
                && total.and_then(std::num::NonZeroU64::new).is_some()
                && failed.is_some()
                && failed_any.is_none()
        })
        && capture_errors.is_empty()
        && missing.is_empty()
        && crashes.is_empty();
    let required = run.pointer("/conditions/record").and_then(Value::as_bool) == Some(true);
    // A capture is media whose kind is an artifact kind: the one mapping the
    // receipt signs media by.
    let present = !required
        || media.iter().any(|item| {
            item.get("kind")
                .and_then(Value::as_str)
                .and_then(crate::manifest::ArtifactKind::of_report_media)
                .is_some()
        });
    let mut errors: Vec<Value> = match analysis_error {
        Some(error) => vec![json!(error)],
        None => Vec::new(),
    };
    if analysis_error.is_none()
        && analysis
            .and_then(|value| value.get("runId"))
            .and_then(Value::as_str)
            != Some(&run_id)
    {
        errors.push(json!("analysis run ID mismatch"));
    }
    if analysis_error.is_none() {
        match total {
            None => errors.push(json!("the analysis states no executed-check total")),
            Some(0) => errors.push(json!("zero executed checks")),
            Some(_) => {}
        }
        if failed.is_none() {
            errors.push(json!("the analysis states no failed-check count"));
        }
        if let Some(failed) = failed_any {
            errors.push(json!(format!("{failed} failed checks")));
        }
    }
    errors.extend(capture_errors.clone());
    errors.extend(missing.iter().map(|item| {
        json!(match item.get("file").and_then(Value::as_str) {
            Some(file) => format!("missing report-typed artifact: {file}"),
            None => format!("missing report-typed artifact that names no file: {item}"),
        })
    }));
    if !present {
        errors.push(json!(
            "recording requested but no report-typed capture was produced"
        ));
    }
    errors.extend(crashes.iter().map(|item| {
        json!(match item
            .get("message")
            .or_else(|| item.get("source"))
            .and_then(Value::as_str)
        {
            Some(said) => format!("crash evidence: {said}"),
            None => format!("crash evidence that states neither message nor source: {item}"),
        })
    }));
    let report_valid = run.pointer("/reportValidation/ok").and_then(Value::as_bool) == Some(true);
    let evidence = json!({"report":report_valid,"analysis":valid,"captureRequired":required,"capturePresent":present,"captureErrors":capture_errors,"missingMedia":missing.iter().filter_map(|item|item.get("file").cloned()).collect::<Vec<_>>(),"crashes":crashes,"errors":errors});
    let passed = run.get("passed").and_then(Value::as_bool) == Some(true)
        && report_valid
        && valid
        && present;
    // Product evidence must belong to this run before it can outrank cleanup.
    let product_failed = analysis.is_some_and(|value| {
        value.get("runId").and_then(Value::as_str) == Some(&run_id)
            && (failed_any.is_some() || !crashes.is_empty())
    });
    let origin = match run.get("failureOrigin").and_then(Value::as_str) {
        _ if passed => Value::Null,
        _ if product_failed => json!("product"),
        Some(declared) => json!(declared),
        None => Value::Null,
    };
    update_json(
        &manifest_path,
        &json!({"status":if passed{"passed"}else{"failed"},"completedAt":now_iso(),"exitCode":run.get("exitCode"),"reportValidation":run.get("reportValidation"),"evidence":evidence,"failure":Value::Null,"failureOrigin":origin,"analysisPath":analysis_path,"artifacts":artifact_hashes(&artifacts,&manifest_path)?}),
    )?;
    let object = run
        .as_object_mut()
        .ok_or_else(|| fail("run.complete", format!("run {run_id} is not a JSON object")))?;
    object.insert("passed".into(), json!(passed));
    object.insert("failureOrigin".into(), origin);
    object.insert("analysisPath".into(), json!(analysis_path));
    object.insert("evidence".into(), evidence);
    Ok(run)
}

/// A finished run analysed and completed: its report is read from the
/// `reportPath` the run recorded and its capture from `artifactsDir`. A run
/// that recorded no report path completes with that refusal as its analysis
/// error, never with a report read from an empty path. Answers the analysis
/// (or its error) and the completed run.
pub(crate) fn analyze_and_complete(
    run: Value,
    frames: Option<f64>,
) -> Result<(Value, Value), Failure> {
    let analyzed = match run.get("reportPath").and_then(Value::as_str) {
        Some(report) => crate::run::analyze_run(
            Path::new(report),
            run.get("artifactsDir")
                .and_then(Value::as_str)
                .map(Path::new),
            run.get("tool").and_then(Value::as_str),
            frames,
            run.get("runId").and_then(Value::as_str),
        ),
        None => Err(fail("run.analyze", "the run recorded no reportPath")),
    };
    match analyzed {
        Ok(analysis) => {
            let completed = complete_run(run, Some(&analysis), None)?;
            Ok((analysis, completed))
        }
        Err(error) => {
            let completed = complete_run(run, None, Some(&error.detail))?;
            Ok((json!({ "error": error.detail }), completed))
        }
    }
}
