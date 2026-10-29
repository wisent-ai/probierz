use crate::stado::*;
use serde_json::json;
pub(crate) fn submit_remote_author(
    harness: &Path,
    app_id: &str,
    journey: &str,
    target: &str,
    description: &str,
    area: &str,
    remote: RemoteJob<'_>,
) -> Result<Value, Failure> {
    let RemoteJob {
        host: host_name,
        mut provision,
        app_repo,
        watch,
    } = remote;
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
    // The router the surface's conditions name, else the one this process
    // is given; a variable that is not Unicode is refused naming it.
    let declared_router = application
        .document
        .get("surfaces")
        .and_then(|value| value.get(target))
        .and_then(|value| value.get("conditions"))
        .and_then(|value| value.get("STADO_MODEL_ROUTER_URL"))
        .and_then(serde_yaml::Value::as_str)
        .map(str::to_string);
    let configured_router = match declared_router {
        Some(router) => Some(router),
        None => match std::env::var("STADO_MODEL_ROUTER_URL") {
            Ok(router) => Some(router),
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => {
                return Err(Failure::config(
                    "stado.submit",
                    format!("STADO_MODEL_ROUTER_URL cannot be read: {error}"),
                ))
            }
        },
    };
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
    // The authoring model this process names is forwarded to the worker; a
    // process that names none forwards none.
    let environment = match std::env::var("PROBIERZ_MODEL") {
        Ok(value) => vec![("PROBIERZ_MODEL".to_string(), value)],
        Err(std::env::VarError::NotPresent) => Vec::new(),
        Err(error) => {
            return Err(Failure::config(
                "stado.submit",
                format!("PROBIERZ_MODEL cannot be read: {error}"),
            ))
        }
    };
    let script = run_script(
        ScriptPlan {
            target,
            app_id,
            spec: None,
            mode: ScriptMode::Author,
            author: Some(author),
            model_router_url: Some(&router),
            record: false,
            environment: &environment,
        },
        provision.as_ref(),
        &packed.hash,
        selected.platform.as_deref(),
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
    )?;
    let identity_fields = copy_submission_identity(
        &identity,
        provision.as_ref(),
        &submission.receipt_dir,
        &inputs,
    )?;
    let mut result = Map::new();
    result.insert("host".into(), json!(host_name));
    result.insert("jobId".into(), json!(submission.job_id));
    result.insert("target".into(), json!(target));
    result.insert("appId".into(), json!(app_id));
    result.insert("journey".into(), json!(journey));
    result.insert("area".into(), json!(area));
    result.insert("submitted".into(), json!(submission.job_id.is_some()));
    result.extend(identity_fields);
    let Some(job_id) = submission.job_id else {
        result.insert("state".into(), Value::String("submit-failed".into()));
        // A submission that failed without saying why carries no failure.
        let failure = match submission.failure {
            Some(failure) => failure,
            None => Value::Null,
        };
        result.insert("failure".into(), failure);
        return Ok(Value::Object(result));
    };
    // Authored specs land in the surface's test directory: the one it
    // declares, or `tests`, the directory a surface uses when it names none.
    let declared_tests = application
        .document
        .get("surfaces")
        .and_then(|value| value.get(target))
        .and_then(|value| value.get("testDirectory"))
        .and_then(serde_yaml::Value::as_str);
    let test_directory = match declared_tests {
        Some(directory) => directory,
        None => "tests",
    };
    let source_receipt = save_author_submission(
        harness,
        &job_id,
        AuthoredJourney {
            app_id,
            journey: &journey,
            area: &area,
            target,
        },
        &product_root,
        test_directory,
        &identity,
    )?;
    result.insert(
        "sourceReceipt".into(),
        Value::String(source_receipt.display().to_string()),
    );
    if !watch {
        result.insert("state".into(), Value::String("queued".into()));
        result.insert("failure".into(), Value::Null);
        return Ok(Value::Object(result));
    }
    let watched = watch_job(&job_id)?;
    let left_evidence = watched_left_evidence(&watched);
    let completed = watched_completed(&watched);
    if let Value::Object(watched) = watched {
        result.extend(watched);
    }
    if left_evidence {
        let retained = fetch_run_evidence(harness, &job_id)?;
        if let Some(path) = &retained.results_dir {
            result.insert(
                "resultsDir".into(),
                Value::String(path.display().to_string()),
            );
        }
        if let Some(error) = &retained.artifact_error {
            result.insert("artifactError".into(), error.clone());
        }
        if completed && retained.results_dir.is_none() {
            result.insert("state".into(), Value::String("evidence-unavailable".into()));
            result.insert(
                "failure".into(),
                missing_evidence(&job_id, "artifact_error=null"),
            );
        } else if completed {
            if let Some(Value::Object(authored)) =
                restore_remote_authoring(harness, &job_id, &retained, Some(app_id), true)?
            {
                result.extend(authored);
            }
            result.insert("specDir".into(), json!(registration_directory(target)));
        }
    }
    Ok(Value::Object(result))
}
