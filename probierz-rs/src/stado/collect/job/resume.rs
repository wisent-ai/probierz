use serde_json::json;
use crate::stado::*;
pub(crate) fn resume_remote_run(
    harness: &Path,
    job_id: Option<&str>,
    host_name: &str,
) -> Result<Value, Failure> {
    let job_id = job_id.unwrap_or("");
    if !safe_job_identifier(job_id) {
        return Err(Failure::config(
            "stado.watch",
            "Resuming remote evidence needs a valid existing Stado job ID.",
        ));
    }
    let selected = host(host_name, "stado.watch")?;
    let watched = watch_job(harness, job_id, &selected, None)?;
    let mut result = json!({
        "host": host_name,
        "jobId": job_id,
        "submitted": false,
    });
    result
        .as_object_mut()
        .expect("object")
        .extend(watched.as_object().cloned().unwrap_or_default());
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !matches!(state.as_str(), "completed" | "failed") {
        return Ok(result);
    }
    let retained = fetch_run_evidence(harness, job_id, &selected)?;
    if let Some(path) = &retained.results_dir {
        result.as_object_mut().expect("object").insert(
            "resultsDir".into(),
            Value::String(path.display().to_string()),
        );
    }
    if let Some(error) = &retained.artifact_error {
        result
            .as_object_mut()
            .expect("object")
            .insert("artifactError".into(), error.clone());
    }
    if let Some(run) = &retained.manifest {
        let object = result.as_object_mut().expect("object");
        for (output, input) in [("runId", "runId"), ("appId", "appId"), ("target", "target")] {
            object.insert(
                output.into(),
                run.get(input).cloned().unwrap_or(Value::Null),
            );
        }
        let authored = restore_remote_authoring(
            harness,
            job_id,
            &retained,
            run.get("appId").and_then(Value::as_str),
            state == "completed" && read_author_submission(harness, job_id).is_some(),
        )?;
        if let Some(Value::Object(authored)) = authored {
            object.extend(authored);
        }
    } else if state == "completed" {
        let object = result.as_object_mut().expect("object");
        object.insert("state".into(), Value::String("evidence-unavailable".into()));
        object.insert(
            "failure".into(),
            missing_evidence(
                job_id,
                &format!(
                    "artifact_error={}",
                    retained.artifact_error.as_ref().unwrap_or(&Value::Null),
                ),
            ),
        );
    }
    Ok(result)
}

pub(crate) fn canonical_job_id(value: &str) -> bool {
    value.len() == 28
        && value.starts_with("job-")
        && value[4..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(crate) fn safe_job_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

