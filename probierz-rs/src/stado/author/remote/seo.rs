use serde_json::json;
use crate::stado::*;
pub(crate) fn uuid_v4() -> Result<String, Failure> {
    let mut bytes = [0_u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let encoded = hex::encode(bytes);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &encoded[0..8],
        &encoded[8..12],
        &encoded[12..16],
        &encoded[16..20],
        &encoded[20..32],
    ))
}

pub(crate) fn submit_remote_seo(harness: &Path, app_id: &str, args: SeoArgs) -> Result<Value, Failure> {
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
    let signature = profile
        .get("requireSignature")
        .and_then(serde_yaml::Value::as_bool)
        .unwrap_or(false);
    let needs_production = profile
        .get("requireProductionEvidence")
        .and_then(serde_yaml::Value::as_bool)
        .unwrap_or(false);
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
    let router = model_router_url(std::env::var("STADO_MODEL_ROUTER_URL").ok().as_deref())?;
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
    let watch_budget = conservative_watch_budget(harness, app_id)?;
    let submission = submit_machine(
        harness,
        &selected,
        &packed.hash,
        "seo",
        inputs,
        secrets,
        watch_budget,
    )?;
    let mut result = json!({
        "host": args.host,
        "jobId": submission.job_id,
        "appId": app_id,
        "mode": args.mode,
        "submitted": submission.job_id.is_some(),
        "watchBudgetMs": submission.watch_budget_ms,
    });
    let Some(job_id) = submission.job_id else {
        let object = result.as_object_mut().expect("object");
        object.insert("state".into(), Value::String("submit-failed".into()));
        object.insert("failure".into(), submission.failure.unwrap_or(Value::Null));
        return Ok(result);
    };
    if args.no_watch {
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
        }
    }
    Ok(result)
}

/// Inputs for the dedicated Byk iOS host bridge. The OTP broker stays local;
/// Stado carries an authenticated loopback bridge to the worker's protected socket.
#[derive(Debug)]
pub struct RemoteBykRequest<'a> {
    pub host_selector: &'a str,
    pub root: &'a Path,
    pub app_path: &'a Path,
    pub ios_device: &'a str,
    pub ios_version: &'a str,
    pub socket_path: &'a Path,
    pub recipient: &'a str,
}

#[derive(Debug)]
pub struct RemoteBykOutcome {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

pub(crate) const BYK_RETRIES: usize = 3;
pub(crate) const BYK_QUARANTINE: Duration = Duration::from_secs(15 * 60);

