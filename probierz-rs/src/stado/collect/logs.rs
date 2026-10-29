use crate::stado::*;
use serde_json::{json, Map};
pub(crate) fn collect_remote_run(
    harness: &Path,
    job_id: Option<&str>,
    app_id: &str,
    host_name: &str,
) -> Result<Value, Failure> {
    let (Some(job_id), Some(_known)) = (
        job_id.filter(|id| canonical_job_id(id)),
        discovery::stado_host(host_name),
    ) else {
        return Err(Failure::config(
            "stado.download",
            "Collection requires a canonical Stado job ID and a known Stado host.",
        ));
    };
    manifest::load(harness, app_id)?;
    let status = sh(
        STADO_BIN,
        &["machine".into(), "status".into(), job_id.into()],
        None,
    );
    if !status.succeeded {
        return Err(remote_failure(
            "stado.watch",
            &format!("Reading job {job_id} failed"),
            &status,
        ));
    }
    let payload: Value = serde_json::from_str(&status.stdout).map_err(|error| {
        remote_failure(
            "stado.watch",
            &format!("Reading job {job_id} returned invalid status ({error})"),
            &status,
        )
    })?;
    // Stado's success, the job, its state and whether it can still move are
    // read together: a status without any of them is refused, never read as
    // an empty state or a job still running.
    let (Some(true), Some(job), Some(said), Some(terminal)) = (
        payload.get("ok").and_then(Value::as_bool),
        payload.pointer("/result/job"),
        payload
            .pointer("/result/job/state")
            .and_then(Value::as_str)
            .filter(|said| !said.is_empty()),
        payload
            .pointer("/result/job/terminal")
            .and_then(Value::as_bool),
    ) else {
        return Err(remote_failure(
            "stado.watch",
            &format!("Reading job {job_id} returned no state or no terminal flag"),
            &status,
        ));
    };
    let job = job.clone();
    let state = said.to_ascii_lowercase();
    let mut result = Map::from_iter([
        ("host".to_string(), Value::from(host_name)),
        ("jobId".to_string(), Value::from(job_id)),
        ("appId".to_string(), Value::from(app_id)),
        ("state".to_string(), Value::from(state.as_str())),
        ("submitted".to_string(), Value::Bool(true)),
        ("collected".to_string(), Value::Bool(false)),
        (
            "source".to_string(),
            Value::from(job.pointer("/resolved_input_artifacts/source").cloned()),
        ),
        ("job".to_string(), job.clone()),
    ]);
    if !terminal {
        return Ok(Value::Object(result));
    }
    // An uploaded job is a completed one whose results are already kept.
    let ended = if state == "uploaded" {
        "completed"
    } else {
        state.as_str()
    };
    if state == "cancelled" && matches!(job.get("started_at"), None | Some(Value::Null)) {
        result.insert("failure".into(), terminal_failure(job_id, &state, &job));
        result.insert("evidence".into(), json!({
            "required": false, "collected": false, "reason": "cancelled-before-start", "retryable": false,
        }));
        return Ok(Value::Object(result));
    }
    let retained = fetch_run_evidence(harness, job_id)?;
    let manifest_matches = retained
        .manifest
        .as_ref()
        .and_then(|value| value.get("appId"))
        .and_then(Value::as_str)
        == Some(app_id);
    if !manifest_matches {
        result.insert(
            "state".into(),
            Value::from(if ended == "completed" {
                "evidence-unavailable"
            } else {
                ended
            }),
        );
        result.insert(
            "artifactError".into(),
            Value::from(retained.artifact_error.clone()),
        );
        result.insert(
            "failure".into(),
            if ended == "completed" {
                missing_evidence(
                    job_id,
                    &format!(
                        "app={app_id}; artifact_error={}",
                        Value::from(retained.artifact_error.clone())
                    ),
                )
            } else {
                terminal_failure(job_id, ended, &job)
            },
        );
        return Ok(Value::Object(result));
    }
    let authored = restore_remote_authoring(
        harness,
        job_id,
        &retained,
        Some(app_id),
        ended == "completed" && read_author_submission(harness, job_id).is_some(),
    )?;
    result.insert("state".into(), Value::from(ended));
    result.insert("collected".into(), Value::Bool(true));
    result.insert(
        "resultsDir".into(),
        Value::from(retained.results_dir.map(|path| path.display().to_string())),
    );
    result.insert("manifest".into(), Value::from(retained.manifest));
    if let Some(Value::Object(authored)) = authored {
        result.extend(authored);
    }
    Ok(Value::Object(result))
}

pub(crate) fn capture_remote_logs(
    job_id: &str,
    directory: &Path,
) -> Result<(PathBuf, PathBuf, Option<Value>), Failure> {
    let log_path = directory.join("command.log");
    let receipts = directory.join("log-receipts.jsonl");
    fs::write(&log_path, [])?;
    fs::write(&receipts, [])?;
    // Named with neither a cursor nor a limit, `stado machine logs JOB_ID`
    // reads the whole log from its start, so one read is the log.
    let page = sh(
        STADO_BIN,
        &["machine".into(), "logs".into(), job_id.into()],
        None,
    );
    append_line(&receipts, page.stdout.trim())?;
    let failed = |action: String| {
        let failure = remote_failure("stado.download", &action, &page);
        Some(failure_summary(&failure, format!("{action}.")))
    };
    let payload: Value = match serde_json::from_str(&page.stdout) {
        Ok(value) => value,
        Err(error) => {
            let action =
                format!("Reading logs for job {job_id} returned invalid metadata ({error})");
            return Ok((log_path, receipts, failed(action)));
        }
    };
    if !page.succeeded || payload.get("ok").and_then(Value::as_bool) != Some(true) {
        let action = format!("Reading logs for job {job_id} failed");
        return Ok((log_path, receipts, failed(action)));
    }
    let (Some(text), Some(true)) = (
        payload.pointer("/result/text").and_then(Value::as_str),
        payload.pointer("/result/eof").and_then(Value::as_bool),
    ) else {
        let action = format!("Reading logs for job {job_id} returned no whole log");
        return Ok((log_path, receipts, failed(action)));
    };
    fs::write(&log_path, text)?;
    Ok((log_path, receipts, None))
}

pub(crate) fn append_line(path: &Path, text: &str) -> Answer {
    let mut file = OpenOptions::new().append(true).open(path)?;
    file.write_all(text.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}
