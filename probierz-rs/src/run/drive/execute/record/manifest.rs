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
    let artifacts = PathBuf::from(
        run.get("artifactsDir")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let manifest_path = PathBuf::from(
        run.get("manifestPath")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let run_id = run
        .get("runId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let analysis_path = artifacts.join("analysis.json");
    let payload = if let Some(error) = analysis_error {
        json!({"runId":run_id,"error":error})
    } else {
        let mut value = analysis.cloned().unwrap_or_else(|| json!({}));
        value
            .as_object_mut()
            .expect("object")
            .insert("runId".into(), json!(run_id));
        value
    };
    write_json(&analysis_path, &payload)?;
    let capture_errors = analysis
        .and_then(|value| value.get("captureErrors"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let media = analysis
        .and_then(|value| value.get("media"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let missing: Vec<Value> = media
        .iter()
        .filter(|item| item.get("missing").and_then(Value::as_bool) == Some(true))
        .cloned()
        .collect();
    let crashes = analysis
        .and_then(|value| value.pointer("/diagnostics/crashes"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
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
    let mut errors: Vec<Value> = analysis_error
        .map(|error| vec![json!(error)])
        .unwrap_or_default();
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
            Some(total) if std::num::NonZeroU64::new(total).is_none() => {
                errors.push(json!("zero executed checks"))
            }
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
        json!(format!(
            "missing report-typed artifact: {}",
            item.get("file").and_then(Value::as_str).unwrap_or("")
        ))
    }));
    if !present {
        errors.push(json!(
            "recording requested but no report-typed capture was produced"
        ));
    }
    errors.extend(crashes.iter().map(|item| {
        json!(format!(
            "crash evidence: {}",
            item.get("message")
                .or_else(|| item.get("source"))
                .and_then(Value::as_str)
                .unwrap_or("unknown crash")
        ))
    }));
    let evidence = json!({"report":run.pointer("/reportValidation/ok").and_then(Value::as_bool).unwrap_or(false),"analysis":valid,"captureRequired":required,"capturePresent":present,"captureErrors":capture_errors,"missingMedia":missing.iter().filter_map(|item|item.get("file").cloned()).collect::<Vec<_>>(),"crashes":crashes,"errors":errors});
    let passed = run.get("passed").and_then(Value::as_bool).unwrap_or(false)
        && evidence.get("report").and_then(Value::as_bool) == Some(true)
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
    let object = run.as_object_mut().expect("object");
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
