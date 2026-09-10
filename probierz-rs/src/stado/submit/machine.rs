use serde_json::json;
use crate::stado::*;
pub(crate) fn submit_machine(
    harness: &Path,
    selected: &discovery::Host,
    hash: &str,
    kind: &str,
    input_objects: Map<String, Value>,
    secret_env: Value,
    watch_budget_ms: u64,
) -> Result<Submission, Failure> {
    if watch_budget_ms == 0 {
        return Err(Failure::config(
            "stado.submit",
            "remote submission requires a positive watch budget",
        ));
    }
    let receipt_dir = harness
        .join("test-results")
        .join(".remote")
        .join(format!("probierz-{kind}-{hash}"));
    fs::create_dir_all(&receipt_dir)?;
    let request_file = receipt_dir.join("request.json");
    let mut request = Map::new();
    request.insert(
        "client_request_id".into(),
        Value::String(format!("probierz-{kind}-{hash}")),
    );
    request.insert(
        "command".into(),
        Value::String(format!(
            "{WATCH_BUDGET_ENV}={watch_budget_ms} bash inputs/run.sh"
        )),
    );
    request.insert("output_uri".into(), Value::String(state_uri("results")));
    request.insert("input_objects".into(), Value::Object(input_objects));
    request.insert("secret_env".into(), secret_env);
    if let Some(extra) = selected.request.as_ref().and_then(Value::as_object) {
        for (name, value) in extra {
            request.insert(name.clone(), value.clone());
        }
    }
    let request = Value::Object(request);
    write_json(&request_file, &request, false, false)?;
    eprintln!(
        "probierz-remote-request {}",
        json!({
            "requestId": request.get("client_request_id"),
            "requestFile": request_file,
        })
    );
    let submit = sh(
        STADO_BIN,
        &[
            "machine".into(),
            "submit".into(),
            "--request-file".into(),
            request_file.display().to_string(),
        ],
        None,
        Some(selected),
        None,
    );
    write_json(
        &receipt_dir.join("submission.json"),
        &process_record(&submit),
        true,
        false,
    )?;
    let payload: Option<Value> = serde_json::from_str(&submit.stdout).ok();
    let mut job_id = payload
        .as_ref()
        .filter(|value| value.get("ok").and_then(Value::as_bool) == Some(true))
        .and_then(|value| value.pointer("/result/job/job_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if job_id.is_none() {
        let message = payload
            .as_ref()
            .and_then(|value| value.pointer("/error/message"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if let Some(candidate) = submitted_job_id(message) {
            let status = sh(
                STADO_BIN,
                &["machine".into(), "status".into(), candidate.clone()],
                None,
                Some(selected),
                Some(STATUS_TIMEOUT),
            );
            if let Ok(value) = serde_json::from_str::<Value>(&status.stdout) {
                if value.get("ok").and_then(Value::as_bool) == Some(true)
                    && value.pointer("/result/job/job_id").and_then(Value::as_str)
                        == Some(candidate.as_str())
                {
                    job_id = Some(candidate);
                }
            }
        }
    }
    if let Some(job_id) = job_id {
        eprintln!(
            "probierz-remote-job {}",
            json!({
                "jobId": job_id,
                "requestId": request.get("client_request_id"),
                "receiptDir": receipt_dir,
            })
        );
        Ok(Submission {
            job_id: Some(job_id),
            watch_budget_ms,
            receipt_dir,
            failure: None,
        })
    } else {
        let failure = remote_failure(
            "stado.submit",
            "The stado queue did not accept the job",
            &submit,
        );
        Ok(Submission {
            job_id: None,
            watch_budget_ms,
            receipt_dir,
            failure: Some(failure_summary(
                &failure,
                "The stado queue did not accept the job.",
            )),
        })
    }
}

pub(crate) fn submitted_job_id(message: &str) -> Option<String> {
    let prefix = "job ";
    let suffix = " was submitted but its idempotency record could not be finalized:";
    let rest = message.strip_prefix(prefix)?;
    let (candidate, _) = rest.split_once(suffix)?;
    (candidate.len() >= 8 && candidate.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| candidate.to_string())
}

pub(crate) fn copy_submission_identity(
    identity: &Identity,
    provision: Option<&Provision>,
    receipt_dir: &Path,
    input_objects: &Map<String, Value>,
) -> Result<Map<String, Value>, Failure> {
    let source_identity_path = receipt_dir.join("source-identity.json");
    fs::copy(&identity.file, &source_identity_path)?;
    let source_revision = identity
        .document
        .pointer("/app/repositories")
        .and_then(Value::as_array)
        .and_then(|repositories| {
            repositories
                .iter()
                .find(|value| value.get("index").and_then(Value::as_u64) == Some(0))
        })
        .and_then(|value| value.get("gitSha"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut values = Map::new();
    values.insert(
        "receiptDir".into(),
        Value::String(receipt_dir.display().to_string()),
    );
    values.insert(
        "sourceIdentityPath".into(),
        Value::String(source_identity_path.display().to_string()),
    );
    values.insert(
        "sourceRevision".into(),
        source_revision
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    if let Some(Provision::NativeBinary {
        binary_name,
        binary_sha256,
        ..
    }) = provision
    {
        let binary = json!({
            "name": binary_name,
            "sha256": binary_sha256,
            "sourceRevision": source_revision,
            "input": input_objects.get("binary").and_then(|value| value.get("relative_path")).cloned().unwrap_or(Value::Null),
        });
        let binary_path = receipt_dir.join("binary-identity.json");
        write_json(&binary_path, &binary, true, true)?;
        values.insert("binary".into(), binary);
        values.insert(
            "binaryIdentityPath".into(),
            Value::String(binary_path.display().to_string()),
        );
    }
    Ok(values)
}

