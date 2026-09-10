use serde_json::json;
use crate::stado::*;
pub(crate) fn submit_remote_author(
    harness: &Path,
    app_id: &str,
    journey: &str,
    target: &str,
    description: &str,
    area: &str,
    host_name: &str,
    mut provision: Option<Provision>,
    app_repo: Option<&Path>,
    watch: bool,
) -> Result<Value, Failure> {
    let journey = safe_author_name(journey, "journey")?;
    let area = safe_author_name(area, "authoring area")?;
    if registration_directory(target).is_none() {
        return Err(Failure::config(
            "stado.submit",
            format!("Remote authoring does not support target \"{target}\"."),
        ));
    }
    let selected = host(host_name, "stado.submit")?;
    require_gui_ready(target, &selected)?;
    let application = manifest::load(harness, app_id)?;
    let configured_router = application
        .document
        .get("surfaces")
        .and_then(|value| value.get(target))
        .and_then(|value| value.get("conditions"))
        .and_then(|value| value.get("STADO_MODEL_ROUTER_URL"))
        .and_then(serde_yaml::Value::as_str)
        .map(str::to_string)
        .or_else(|| std::env::var("STADO_MODEL_ROUTER_URL").ok());
    let router = model_router_url(configured_router.as_deref())?;
    let product_root = repository_root(harness, app_id, app_repo)?;
    let identity = pack_source_identity(harness, app_id, Some(&product_root))?;
    require_immutable_native(target, provision.as_ref(), app_repo, &identity)?;
    let packed = pack_repo(harness, &[app_id])?;
    let repo_uri = upload(&packed.file, &format!("probierz-{}.tar.gz", packed.hash))?;
    let identity_uri = upload(
        &identity.file,
        &format!("source-{app_id}-{}.json", identity.hash),
    )?;
    let provisioned = provision_inputs(app_id, &mut provision, Some(&product_root), true)?;
    let receipt_id = format!("remote-{}", uuid_v4()?);
    let author = (
        journey.as_str(),
        area.as_str(),
        description,
        receipt_id.as_str(),
    );
    let environment = std::env::var("PROBIERZ_MODEL")
        .ok()
        .map(|value| vec![("PROBIERZ_MODEL".to_string(), value)])
        .unwrap_or_default();
    let script = run_script(
        target,
        app_id,
        None,
        provision.as_ref(),
        &packed.hash,
        selected.platform,
        "author",
        Some(author),
        Some(&router),
        false,
        &environment,
    )?;
    let script_file = work_path(&format!("probierz-author-{}.sh", packed.hash))?;
    fs::write(&script_file, script)?;
    let script_uri = upload(&script_file, &format!("author-{}.sh", packed.hash))?;
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
    let watch_budget = conservative_watch_budget(harness, app_id)?;
    let secrets = remote_secret_env(
        harness,
        app_id,
        &["STADO_MODEL_ROUTER_TOKEN", "PROBIERZ_MODEL_AGENT_SECRET"],
    )?;
    let submission = submit_machine(
        harness,
        &selected,
        &packed.hash,
        "author",
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
        "journey": journey,
        "area": area,
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
    let test_directory = application
        .document
        .get("surfaces")
        .and_then(|value| value.get(target))
        .and_then(|value| value.get("testDirectory"))
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("tests");
    let source_receipt = save_author_submission(
        harness,
        &job_id,
        app_id,
        &journey,
        &area,
        target,
        &product_root,
        test_directory,
        &identity,
    )?;
    result.as_object_mut().expect("object").insert(
        "sourceReceipt".into(),
        Value::String(source_receipt.display().to_string()),
    );
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
        if state == "completed" && retained.results_dir.is_none() {
            let object = result.as_object_mut().expect("object");
            object.insert("state".into(), Value::String("evidence-unavailable".into()));
            object.insert(
                "failure".into(),
                missing_evidence(&job_id, "artifact_error=null"),
            );
        } else if state == "completed" {
            if let Some(Value::Object(authored)) =
                restore_remote_authoring(harness, &job_id, &retained, Some(app_id), true)?
            {
                result.as_object_mut().expect("object").extend(authored);
            }
            result.as_object_mut().expect("object").insert(
                "specDir".into(),
                registration_directory(target)
                    .map(Value::from)
                    .unwrap_or(Value::Null),
            );
        }
    }
    Ok(result)
}

