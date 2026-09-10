use serde_json::json;
use crate::stado::*;
pub(crate) fn collect_remote_run(
    harness: &Path,
    job_id: Option<&str>,
    app_id: &str,
    host_name: &str,
) -> Result<Value, Failure> {
    let job_id = job_id.unwrap_or("");
    let selected = discovery::stado_host(host_name);
    if !canonical_job_id(job_id) || selected.is_none() {
        return Err(Failure::config(
            "stado.download",
            "Collection requires a canonical Stado job ID and a known Stado host.",
        ));
    }
    let selected = selected.expect("checked");
    manifest::load(harness, app_id)?;
    let status = sh(
        STADO_BIN,
        &["machine".into(), "status".into(), job_id.into()],
        None,
        Some(&selected),
        Some(STATUS_TIMEOUT),
    );
    if status.status != Some(0) {
        return Err(remote_failure(
            "stado.watch",
            &format!("Reading job {job_id} failed"),
            &status,
        ));
    }
    let payload: Value = serde_json::from_str(&status.stdout).map_err(|_| {
        remote_failure(
            "stado.watch",
            &format!("Reading job {job_id} returned invalid status"),
            &status,
        )
    })?;
    let job = payload
        .pointer("/result/job")
        .cloned()
        .unwrap_or(Value::Null);
    let state = job
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if payload.get("ok").and_then(Value::as_bool) != Some(true) || state.is_empty() {
        return Err(remote_failure(
            "stado.watch",
            &format!("Reading job {job_id} returned no state"),
            &status,
        ));
    }
    let mut result = json!({
        "host": host_name,
        "jobId": job_id,
        "appId": app_id,
        "state": state,
        "submitted": true,
        "collected": false,
        "job": job,
        "source": job.pointer("/resolved_input_artifacts/source").cloned().unwrap_or(Value::Null),
    });
    if !matches!(
        state.as_str(),
        "uploaded" | "completed" | "failed" | "cancelled"
    ) {
        return Ok(result);
    }
    if state == "cancelled" && job.get("started_at").map(Value::is_null).unwrap_or(true) {
        let object = result.as_object_mut().expect("object");
        object.insert("failure".into(), terminal_failure(job_id, &state, &job));
        object.insert("evidence".into(), json!({
            "required": false, "collected": false, "reason": "cancelled-before-start", "retryable": false,
        }));
        return Ok(result);
    }
    let retained = fetch_run_evidence(harness, job_id, &selected)?;
    let manifest_matches = retained
        .manifest
        .as_ref()
        .and_then(|value| value.get("appId"))
        .and_then(Value::as_str)
        == Some(app_id);
    if !manifest_matches {
        let terminal = if state == "uploaded" {
            "completed"
        } else {
            state.as_str()
        };
        let object = result.as_object_mut().expect("object");
        object.insert(
            "state".into(),
            Value::String(
                if terminal == "completed" {
                    "evidence-unavailable"
                } else {
                    terminal
                }
                .to_string(),
            ),
        );
        object.insert(
            "artifactError".into(),
            retained.artifact_error.clone().unwrap_or(Value::Null),
        );
        object.insert(
            "failure".into(),
            if terminal == "completed" {
                missing_evidence(
                    job_id,
                    &format!(
                        "app={app_id}; artifact_error={}",
                        retained.artifact_error.as_ref().unwrap_or(&Value::Null)
                    ),
                )
            } else {
                terminal_failure(job_id, terminal, &job)
            },
        );
        return Ok(result);
    }
    let authored = restore_remote_authoring(
        harness,
        job_id,
        &retained,
        Some(app_id),
        matches!(state.as_str(), "uploaded" | "completed")
            && read_author_submission(harness, job_id).is_some(),
    )?;
    let object = result.as_object_mut().expect("object");
    object.insert(
        "state".into(),
        Value::String(if state == "uploaded" {
            "completed".into()
        } else {
            state
        }),
    );
    object.insert("collected".into(), Value::Bool(true));
    object.insert(
        "resultsDir".into(),
        retained
            .results_dir
            .map(|path| Value::String(path.display().to_string()))
            .unwrap_or(Value::Null),
    );
    object.insert("manifest".into(), retained.manifest.unwrap_or(Value::Null));
    if let Some(Value::Object(authored)) = authored {
        object.extend(authored);
    }
    Ok(result)
}

pub(crate) fn capture_remote_logs(
    job_id: &str,
    selected: &discovery::Host,
    directory: &Path,
) -> Result<(PathBuf, PathBuf, Option<Value>), Failure> {
    let log_path = directory.join("command.log");
    let receipts = directory.join("log-receipts.jsonl");
    fs::write(&log_path, [])?;
    fs::write(&receipts, [])?;
    let mut cursor = 0_u64;
    loop {
        let page = sh(
            STADO_BIN,
            &[
                "machine".into(),
                "logs".into(),
                job_id.into(),
                "--cursor".into(),
                cursor.to_string(),
                "--limit".into(),
                "65536".into(),
            ],
            None,
            Some(selected),
            Some(STATUS_TIMEOUT),
        );
        append_line(&receipts, page.stdout.trim())?;
        let payload: Value = match serde_json::from_str(&page.stdout) {
            Ok(value) => value,
            Err(_) => {
                let failure = remote_failure(
                    "stado.download",
                    &format!("Reading logs for job {job_id} returned invalid metadata"),
                    &page,
                );
                return Ok((
                    log_path,
                    receipts,
                    Some(failure_summary(
                        &failure,
                        format!("Reading logs for job {job_id} returned invalid metadata."),
                    )),
                ));
            }
        };
        if page.status != Some(0) || payload.get("ok").and_then(Value::as_bool) != Some(true) {
            let failure = remote_failure(
                "stado.download",
                &format!("Reading logs for job {job_id} failed"),
                &page,
            );
            return Ok((
                log_path,
                receipts,
                Some(failure_summary(
                    &failure,
                    format!("Reading logs for job {job_id} failed."),
                )),
            ));
        }
        let text = payload
            .pointer("/result/text")
            .and_then(Value::as_str)
            .unwrap_or("");
        OpenOptions::new()
            .append(true)
            .open(&log_path)?
            .write_all(text.as_bytes())?;
        if payload.pointer("/result/eof").and_then(Value::as_bool) == Some(true) {
            return Ok((log_path, receipts, None));
        }
        let next = payload
            .pointer("/result/next_cursor")
            .and_then(Value::as_u64);
        if next.map(|value| value > cursor) != Some(true) {
            let message = format!("Stado returned an invalid log cursor for job {job_id}; the pages received so far were retained.");
            let failure = Failure::new("stado.download", Code::Unknown, &message);
            return Ok((log_path, receipts, Some(failure_summary(&failure, message))));
        }
        cursor = next.expect("checked");
    }
}

pub(crate) fn append_line(path: &Path, text: &str) -> Answer {
    let mut file = OpenOptions::new().append(true).open(path)?;
    file.write_all(text.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

