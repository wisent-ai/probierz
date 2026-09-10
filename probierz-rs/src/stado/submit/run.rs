use serde_json::json;
use crate::stado::*;
pub(crate) fn submit_remote_run(
    harness: &Path,
    target: &str,
    app_id: &str,
    spec: Option<&str>,
    host_name: &str,
    mut provision: Option<Provision>,
    app_repo: Option<&Path>,
    watch: bool,
    mode: &str,
    record: bool,
    environment: &[(String, String)],
) -> Result<Value, Failure> {
    let selected = host(host_name, "stado.submit")?;
    require_gui_ready(target, &selected)?;
    let identity = pack_source_identity(harness, app_id, app_repo)?;
    require_immutable_native(target, provision.as_ref(), app_repo, &identity)?;
    let watch_budget =
        selected_run_budget(harness, app_id, target, environment, provision.as_ref())?;
    let packed = pack_repo(harness, &[app_id])?;
    let repo_uri = upload(&packed.file, &format!("probierz-{}.tar.gz", packed.hash))?;
    let identity_uri = upload(
        &identity.file,
        &format!("source-{app_id}-{}.json", identity.hash),
    )?;
    let provisioned = provision_inputs(app_id, &mut provision, app_repo, false)?;
    let script = run_script(
        target,
        app_id,
        spec,
        provision.as_ref(),
        &packed.hash,
        selected.platform,
        mode,
        None,
        None,
        record,
        environment,
    )?;
    let script_file = work_path(&format!("probierz-run-{}.sh", packed.hash))?;
    fs::write(&script_file, script)?;
    let script_uri = upload(&script_file, &format!("run-{}.sh", packed.hash))?;
    let mut inputs = Map::new();
    inputs.insert(
        "repo".into(),
        json!({ "stado_uri": repo_uri, "relative_path": "inputs/probierz.tar.gz" }),
    );
    inputs.insert(
        "script".into(),
        json!({ "stado_uri": script_uri, "relative_path": "inputs/run.sh" }),
    );
    inputs.insert(
        "source".into(),
        json!({ "stado_uri": identity_uri, "relative_path": "inputs/source-identity.json" }),
    );
    inputs.extend(provisioned);
    let secrets = remote_secret_env(
        harness,
        app_id,
        &["STADO_MODEL_ROUTER_TOKEN", "PROBIERZ_MODEL_AGENT_SECRET"],
    )?;
    let submission = submit_machine(
        harness,
        &selected,
        &packed.hash,
        "run",
        inputs.clone(),
        secrets,
        watch_budget,
    )?;
    let identity_fields = copy_submission_identity(
        &identity,
        provision.as_ref(),
        &submission.receipt_dir,
        &inputs,
    )?;
    let mut result = json!({
        "host": host_name,
        "jobId": submission.job_id,
        "target": target,
        "appId": app_id,
        "submitted": submission.job_id.is_some(),
        "watchBudgetMs": submission.watch_budget_ms,
    });
    result
        .as_object_mut()
        .expect("object")
        .extend(identity_fields);
    let Some(job_id) = submission.job_id else {
        let object = result.as_object_mut().expect("object");
        object.insert("state".into(), Value::String("submit-failed".into()));
        object.insert("failure".into(), submission.failure.unwrap_or(Value::Null));
        return Ok(result);
    };
    if !watch {
        let object = result.as_object_mut().expect("object");
        object.insert("state".into(), Value::String("queued".into()));
        object.insert("failure".into(), Value::Null);
        return Ok(result);
    }
    let watched = watch_job(harness, &job_id, &selected, Some(watch_budget))?;
    result
        .as_object_mut()
        .expect("object")
        .extend(watched.as_object().cloned().unwrap_or_default());
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if matches!(state.as_str(), "completed" | "failed") {
        let retained = fetch_run_evidence(harness, &job_id, &selected)?;
        if let Some(path) = retained.results_dir {
            result.as_object_mut().expect("object").insert(
                "resultsDir".into(),
                Value::String(path.display().to_string()),
            );
        }
        if let Some(error) = retained.artifact_error {
            result
                .as_object_mut()
                .expect("object")
                .insert("artifactError".into(), error);
        }
        if state == "completed" && result.get("resultsDir").is_none() {
            let object = result.as_object_mut().expect("object");
            object.insert("state".into(), Value::String("evidence-unavailable".into()));
            object.insert(
                "failure".into(),
                missing_evidence(&job_id, "artifact_error=null"),
            );
        } else if state == "failed" {
            if let Some(preflight) = retained
                .manifest
                .as_ref()
                .and_then(|value| value.get("preflight"))
                .filter(|value| value.get("ready").and_then(Value::as_bool) == Some(false))
            {
                let missing = preflight
                    .get("missing")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| "target prerequisites".into());
                let remediation = preflight
                    .get("remediation")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .unwrap_or_default();
                let detail = format!(
                    "missing: {missing}{}",
                    if remediation.is_empty() {
                        String::new()
                    } else {
                        format!("; remediation: {remediation}")
                    }
                );
                let failure = Failure::config("stado.worker", detail);
                let object = result.as_object_mut().expect("object");
                object.insert("preflight".into(), preflight.clone());
                object.insert("failure".into(), failure_summary(&failure, format!("Job {job_id} did not execute because the selected host is missing: {missing}.")));
            }
        }
    }
    Ok(result)
}

pub(crate) fn require_immutable_native(
    target: &str,
    provision: Option<&Provision>,
    app_repo: Option<&Path>,
    identity: &Identity,
) -> Answer {
    if !matches!(provision, Some(Provision::NativeBinary { .. })) {
        return Ok(());
    }
    if target != "tui" {
        return Err(Failure::config(
            "stado.submit",
            "--app-binary-path is supported only for remote TUI runs and authoring.",
        ));
    }
    if app_repo.is_none() {
        return Err(Failure::config(
            "stado.pack",
            "Remote native-binary provisioning needs --app-repo <path>.",
        ));
    }
    let primary = identity
        .document
        .pointer("/app/repositories")
        .and_then(Value::as_array)
        .and_then(|repositories| {
            repositories
                .iter()
                .find(|value| value.get("index").and_then(Value::as_u64) == Some(0))
        });
    let clean = primary
        .and_then(|value| value.get("gitSha"))
        .and_then(Value::as_str)
        .is_some()
        && primary
            .and_then(|value| value.get("dirty"))
            .and_then(Value::as_bool)
            == Some(false);
    if !clean {
        return Err(Failure::config(
            "stado.pack",
            "--app-binary-path requires --app-repo to be a clean committed source checkout.",
        ));
    }
    Ok(())
}

pub(crate) fn safe_author_name(value: &str, label: &str) -> Result<String, Failure> {
    let clean = value.trim();
    let mut bytes = clean.bytes();
    let valid = matches!(bytes.next(), Some(byte) if byte.is_ascii_alphanumeric())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if !valid {
        return Err(Failure::config(
            "stado.submit",
            format!("{label} must be one safe path name: {value}"),
        ));
    }
    Ok(clean.to_string())
}

pub(crate) fn repository_root(
    harness: &Path,
    app_id: &str,
    explicit: Option<&Path>,
) -> Result<PathBuf, Failure> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    let application = manifest::load(harness, app_id)?;
    let root = application
        .document
        .get("repositories")
        .and_then(serde_yaml::Value::as_sequence)
        .and_then(|items| items.first())
        .and_then(|value| value.get("root"))
        .and_then(serde_yaml::Value::as_str)
        .ok_or_else(|| {
            Failure::config(
                "stado.pack",
                format!("app {app_id} has no primary repository root"),
            )
        })?;
    Ok(PathBuf::from(root))
}

pub(crate) fn model_router_url(value: Option<&str>) -> Result<String, Failure> {
    let clean = value.unwrap_or("").trim();
    if clean.is_empty() {
        return Err(Failure::config(
            "stado.submit",
            "STADO_MODEL_ROUTER_URL is required",
        ));
    }
    let secure = clean.starts_with("https://");
    let loopback = clean.starts_with("http://localhost")
        || clean.starts_with("http://127.")
        || clean.starts_with("http://[::1]");
    let authority = clean
        .split_once("://")
        .map(|(_, value)| value.split('/').next().unwrap_or(""))
        .unwrap_or("");
    if !secure && !loopback {
        return Err(Failure::config(
            "stado.submit",
            "STADO_MODEL_ROUTER_URL must use HTTPS or loopback HTTP",
        ));
    }
    if authority.contains('@') || clean.contains('?') || clean.contains('#') {
        return Err(Failure::config(
            "stado.submit",
            "STADO_MODEL_ROUTER_URL must not contain credentials, query parameters, or a fragment",
        ));
    }
    Ok(clean.trim_end_matches('/').to_string())
}

