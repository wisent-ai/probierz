use serde_json::json;
use crate::run::*;
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
    let valid = analysis_error.is_none()
        && analysis.is_some_and(|value| {
            value.get("runId").and_then(Value::as_str) == Some(&run_id)
                && js_number(value.get("total")) > 0.0
                && js_number(value.get("failed")) == 0.0
        })
        && capture_errors.is_empty()
        && missing.is_empty()
        && crashes.is_empty();
    let kinds: BTreeSet<&str> = media
        .iter()
        .filter_map(|item| item.get("kind").and_then(Value::as_str))
        .collect();
    let required = run
        .pointer("/conditions/record")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let present = !required
        || ["video", "trace", "screenshot"]
            .iter()
            .any(|kind| kinds.contains(kind));
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
    if analysis_error.is_none()
        && analysis
            .map(|value| js_number(value.get("total")) <= 0.0)
            .unwrap_or(true)
    {
        errors.push(json!("zero executed checks"));
    }
    if analysis_error.is_none()
        && analysis
            .map(|value| js_number(value.get("failed")) > 0.0)
            .unwrap_or(false)
    {
        errors.push(json!(format!(
            "{} failed checks",
            analysis
                .map(|value| js_number(value.get("failed")))
                .unwrap_or(0.0)
        )));
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
    update_json(
        &manifest_path,
        &json!({"status":if passed{"passed"}else{"failed"},"completedAt":now_iso(),"exitCode":run.get("exitCode"),"timedOut":run.get("timedOut"),"reportValidation":run.get("reportValidation"),"evidence":evidence,"failure":Value::Null,"analysisPath":analysis_path,"artifacts":artifact_hashes(&artifacts,&manifest_path)?}),
    )?;
    let object = run.as_object_mut().expect("object");
    object.insert("passed".into(), json!(passed));
    object.insert("analysisPath".into(), json!(analysis_path));
    object.insert("evidence".into(), evidence);
    Ok(run)
}

