use crate::stado::*;
use serde_json::json;

/// A random (version 4) UUID drawn from the system's random device.
pub(crate) fn uuid_v4() -> Result<String, Failure> {
    let mut bytes: uuid::Bytes = Default::default();
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(uuid::Builder::from_random_bytes(bytes).into_uuid().to_string())
}

/// The yes/no `key` of an SEO profile: absent means the profile does not ask
/// for it; any value that is not a boolean is refused naming the key, rather
/// than read as not asked.
fn profile_flag(profile: &serde_yaml::Value, mode: &str, key: &str) -> Result<bool, Failure> {
    match profile.get(key) {
        None => Ok(false),
        Some(value) => value.as_bool().ok_or_else(|| {
            Failure::config(
                "stado.submit",
                format!("the {mode} SEO profile's {key} is {value:?}, not true or false"),
            )
        }),
    }
}

pub(crate) fn submit_remote_seo(
    harness: &Path,
    app_id: &str,
    args: SeoArgs,
) -> Result<Value, Failure> {
    let selected = host(&args.host, "stado.submit")?;
    let (base_url, primary, secondary, adjudicator) = match (
        args.base_url.as_deref(), args.primary_model.as_deref(),
        args.secondary_model.as_deref(), args.adjudicator_model.as_deref(),
    ) {
        (Some(base), Some(primary), Some(secondary), Some(adjudicator)) => (base, primary, secondary, adjudicator),
        _ => return Err(Failure::config(
            "stado.submit",
            "Remote SEO evaluation needs --base-url, --primary-model, --secondary-model, and --adjudicator-model.",
        )),
    };
    let application = manifest::load(harness, app_id)?;
    let profile = application
        .document
        .get("seo")
        .and_then(|value| value.get("profiles"))
        .and_then(|value| value.get(&args.mode))
        .ok_or_else(|| {
            Failure::config(
                "stado.submit",
                format!("app {app_id} has no SEO profile for {}", args.mode),
            )
        })?;
    let signature = profile_flag(profile, &args.mode, "requireSignature")?;
    let needs_production = profile_flag(profile, &args.mode, "requireProductionEvidence")?;
    if needs_production && args.production_evidence.is_none() {
        return Err(Failure::config(
            "stado.submit",
            format!("{} SEO profile requires --production-evidence", args.mode),
        ));
    }
    let policy = args
        .policy
        .as_deref()
        .or_else(|| manifest_string(&application.document, &["seo", "policy"]))
        .ok_or_else(|| Failure::config("stado.submit", "seo.policy is required"))?;
    let brief = args
        .brief
        .as_deref()
        .or_else(|| manifest_string(&application.document, &["seo", "brief"]))
        .ok_or_else(|| Failure::config("stado.submit", "seo.brief is required"))?;
    if let Some(file) = args.production_evidence.as_deref() {
        if !file.exists() {
            return Err(Failure::config(
                "stado.submit",
                format!("production SEO evidence not found: {}", file.display()),
            ));
        }
    }
    let packed = pack_repo(harness, &[app_id])?;
    let repo_uri = upload(&packed.file, &format!("probierz-{}.tar.gz", packed.hash))?;
    // A router setting the environment holds but cannot hand over as text is
    // refused with its cause, not read as never set.
    let configured_router = match std::env::var("STADO_MODEL_ROUTER_URL") {
        Ok(router) => Some(router),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => {
            return Err(Failure::config(
                "stado.submit",
                format!("STADO_MODEL_ROUTER_URL cannot be read: {error}"),
            ))
        }
    };
    let router = model_router_url(configured_router.as_deref())?;
    let script = seo_script(
        app_id,
        base_url,
        &args.mode,
        policy,
        brief,
        primary,
        secondary,
        adjudicator,
        &args.agent_id,
        &router,
        args.production_evidence.is_some(),
        signature,
        &packed.hash,
    );
    let script_file = work_path(&format!("probierz-seo-{}.sh", packed.hash))?;
    fs::write(&script_file, script)?;
    let mut inputs = Map::new();
    inputs.insert(
        "repo".into(),
        json!({ "stado_uri": repo_uri, "relative_path": "inputs/probierz.tar.gz" }),
    );
    inputs.insert(
        "script".into(),
        json!({
            "stado_uri": upload(&script_file, &format!("seo-{}.sh", packed.hash))?,
            "relative_path": "inputs/run.sh",
        }),
    );
    if let Some(file) = args.production_evidence.as_deref() {
        inputs.insert(
            "productionEvidence".into(),
            json!({
                "stado_uri": upload(file, &format!("seo-production-{}.json", packed.hash))?,
                "relative_path": "inputs/production-evidence.json",
            }),
        );
    }
    let mut secret_names = vec!["STADO_MODEL_ROUTER_TOKEN", "PROBIERZ_MODEL_AGENT_SECRET"];
    if signature {
        secret_names.push("PROBIERZ_SEO_RECEIPT_PRIVATE_KEY");
    }
    let secrets = remote_secret_env(harness, app_id, &secret_names)?;
    let submission = submit_machine(harness, &selected, &packed.hash, "seo", inputs, secrets)?;
    let mut result = Map::from_iter([
        ("host".to_owned(), json!(args.host)),
        ("jobId".to_owned(), json!(submission.job_id)),
        ("appId".to_owned(), json!(app_id)),
        ("mode".to_owned(), json!(args.mode)),
        ("submitted".to_owned(), json!(submission.job_id.is_some())),
    ]);
    let Some(job_id) = submission.job_id else {
        result.insert("state".into(), Value::String("submit-failed".into()));
        result.insert("failure".into(), json!(submission.failure));
        return Ok(Value::Object(result));
    };
    if args.no_watch {
        result.insert("state".into(), Value::String("queued".into()));
        result.insert("failure".into(), Value::Null);
        return Ok(Value::Object(result));
    }
    // How the job ended is read from Stado's terminal state, typed in
    // watch.rs, as resume and remote authoring read it.
    let watched = watch_job(&job_id)?;
    let left_evidence = watched_left_evidence(&watched);
    let completed = watched_completed(&watched);
    let Value::Object(watched) = watched else {
        return Err(Failure::invalid("stado.watch", format!("the watch of job {job_id} answered no object")));
    };
    result.extend(watched);
    if !left_evidence {
        return Ok(Value::Object(result));
    }
    let retained = fetch_run_evidence(harness, &job_id)?;
    if let Some(path) = retained.results_dir {
        result.insert("resultsDir".into(), Value::String(path.display().to_string()));
    }
    if let Some(error) = retained.artifact_error {
        result.insert("artifactError".into(), error);
    }
    if completed && result.get("resultsDir").is_none() {
        result.insert("state".into(), Value::String("evidence-unavailable".into()));
        result.insert("failure".into(), missing_evidence(&job_id, "artifact_error=null"));
    }
    Ok(Value::Object(result))
}
