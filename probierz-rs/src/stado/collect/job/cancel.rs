use serde_json::json;
use crate::stado::*;
pub(crate) fn cancel_remote_run(
    harness: &Path,
    job_id: Option<&str>,
    host_name: &str,
    reason: &str,
) -> Result<Value, Failure> {
    let job_id = job_id.unwrap_or("");
    if !canonical_job_id(job_id) {
        return Err(Failure::config(
            "stado.watch",
            "Cancelling remote evidence needs a canonical Stado job ID.",
        ));
    }
    let selected = host(host_name, "stado.watch")?;
    let reason = reason.trim();
    if reason.is_empty() || reason.contains('\0') {
        return Err(Failure::config(
            "stado.watch",
            "Cancelling a remote run needs --reason <reason>.",
        ));
    }
    let requested = Utc::now();
    let attempt_id = format!(
        "{}-{}",
        requested.format("%Y%m%d%H%M%S%3f"),
        &nonce("cancel")[..8]
    );
    let cancellation_root = harness
        .join("test-results")
        .join(".remote")
        .join("cancellations")
        .join(job_id);
    let directory = cancellation_root.join(&attempt_id);
    fs::create_dir_all(&directory)?;
    let request_path = directory.join("request.json");
    write_json(
        &request_path,
        &json!({
            "schemaVersion": 1,
            "jobId": job_id,
            "host": host_name,
            "reason": reason,
            "requestedAt": requested.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        }),
        true,
        true,
    )?;
    let before = sh(
        STADO_BIN,
        &["machine".into(), "status".into(), job_id.into()],
        None,
        Some(&selected),
        Some(STATUS_TIMEOUT),
    );
    let before_path = directory.join("status-before.json");
    fs::write(&before_path, &before.stdout)?;
    write_json(
        &directory.join("status-before-process.json"),
        &process_record(&before),
        true,
        true,
    )?;
    let before_payload: Value = serde_json::from_str(&before.stdout).map_err(|_| {
        remote_failure(
            "stado.watch",
            &format!("Reading the original state for job {job_id} returned invalid metadata"),
            &before,
        )
    })?;
    let original_job = before_payload
        .pointer("/result/job")
        .cloned()
        .unwrap_or(Value::Null);
    if before.status != Some(0)
        || before_payload.get("ok").and_then(Value::as_bool) != Some(true)
        || original_job.is_null()
    {
        return Err(remote_failure(
            "stado.watch",
            &format!("Reading the original state for job {job_id} failed"),
            &before,
        ));
    }
    let cancellation = sh(
        STADO_BIN,
        &["machine".into(), "cancel".into(), job_id.into()],
        None,
        Some(&selected),
        Some(STATUS_TIMEOUT),
    );
    let receipt_path = directory.join("receipt.json");
    fs::write(&receipt_path, &cancellation.stdout)?;
    write_json(
        &directory.join("receipt-process.json"),
        &process_record(&cancellation),
        true,
        true,
    )?;
    let cancellation_payload: Value = serde_json::from_str(&cancellation.stdout).map_err(|_| {
        remote_failure(
            "stado.watch",
            &format!("Cancelling job {job_id} returned an invalid receipt"),
            &cancellation,
        )
    })?;
    let job = cancellation_payload
        .pointer("/result/job")
        .cloned()
        .unwrap_or(Value::Null);
    if cancellation.status != Some(0)
        || cancellation_payload.get("ok").and_then(Value::as_bool) != Some(true)
        || job.is_null()
    {
        return Err(remote_failure(
            "stado.watch",
            &format!("Cancelling job {job_id} failed"),
            &cancellation,
        ));
    }
    let (log_path, log_receipts, log_failure) = capture_remote_logs(job_id, &selected, &directory)?;
    let state = job
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut retained = None;
    let mut evidence_failure = None;
    if job.get("started_at").is_some_and(|value| !value.is_null())
        && matches!(
            state.as_str(),
            "cancelled" | "completed" | "uploaded" | "failed"
        )
    {
        match fetch_run_evidence(harness, job_id, &selected) {
            Ok(value) => retained = Some(value),
            Err(failure) => {
                evidence_failure = Some(failure_summary(&failure, failure.detail.clone()))
            }
        }
    }
    let cancelled = state == "cancelled";
    let required = job.get("started_at").is_some_and(|value| !value.is_null());
    let collected = retained
        .as_ref()
        .and_then(|value| value.results_dir.as_ref())
        .is_some();
    let artifact_error = retained
        .as_ref()
        .and_then(|value| value.artifact_error.clone());
    let evidence = json!({
        "required": required,
        "collected": collected,
        "resultsDir": retained.as_ref().and_then(|value| value.results_dir.as_ref()).map(|path| path.display().to_string()),
        "artifactError": artifact_error,
        "failure": evidence_failure,
        "reason": if required { Value::Null } else { Value::String("cancelled-before-start".into()) },
    });
    let evaluation_failure = if cancelled {
        terminal_failure(job_id, &state, &job)
    } else {
        let message = format!(
            "Job {job_id} is {} and was not cancelled.",
            if state.is_empty() {
                "in an unknown state"
            } else {
                &state
            }
        );
        failure_summary(
            &Failure::new("stado.worker", Code::Unknown, &message),
            message,
        )
    };
    let cancellation_failure = if !cancelled {
        Some(evaluation_failure.clone())
    } else if let Some(value) = log_failure.clone().or(evidence_failure.clone()) {
        Some(value)
    } else if required && !collected {
        let message = format!("Cancellation of job {job_id} succeeded, but its required worker evidence was not retained.");
        Some(failure_summary(
            &Failure::config("stado.download", &message),
            message,
        ))
    } else {
        None
    };
    Ok(json!({
        "host": host_name,
        "jobId": job_id,
        "submitted": false,
        "state": state,
        "cancelled": cancelled,
        "passed": false,
        "cancellationSucceeded": cancelled && cancellation_failure.is_none(),
        "cancellationFailure": cancellation_failure,
        "reason": reason,
        "cancellationRoot": cancellation_root,
        "attemptId": attempt_id,
        "originalJob": original_job,
        "job": job,
        "source": original_job.pointer("/resolved_input_artifacts/source").cloned()
            .or_else(|| job.pointer("/resolved_input_artifacts/source").cloned()).unwrap_or(Value::Null),
        "cancellationDir": directory,
        "requestPath": request_path,
        "statusBeforePath": before_path,
        "receiptPath": receipt_path,
        "logsPath": log_path,
        "logReceiptsPath": log_receipts,
        "logFailure": log_failure,
        "evidence": evidence,
        "resultsDir": retained.and_then(|value| value.results_dir).map(|path| path.display().to_string()),
        "failure": evaluation_failure,
    }))
}

