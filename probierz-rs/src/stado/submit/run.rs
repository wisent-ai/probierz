use crate::stado::*;
use serde_json::json;
use std::num::NonZeroU64;
pub(crate) fn submit_remote_run(
    harness: &Path,
    target: &str,
    app_id: &str,
    spec: Option<&str>,
    host_name: &str,
    mut provision: Option<Provision>,
    app_repo: Option<&Path>,
    watch: bool,
    mode: ScriptMode,
    record: bool,
    environment: &[(String, String)],
) -> Result<Value, Failure> {
    let selected = host(host_name, "stado.submit")?;
    require_gui_ready(target, &selected)?;
    let identity = pack_source_identity(harness, app_id, app_repo)?;
    require_immutable_native(target, provision.as_ref(), app_repo, &identity)?;
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
        selected.platform.as_deref(),
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
    )?;
    let identity_fields = copy_submission_identity(
        &identity,
        provision.as_ref(),
        &submission.receipt_dir,
        &inputs,
    )?;
    let mut result = Map::from_iter([
        ("host".to_owned(), json!(host_name)),
        ("jobId".to_owned(), json!(submission.job_id)),
        ("target".to_owned(), json!(target)),
        ("appId".to_owned(), json!(app_id)),
        ("submitted".to_owned(), json!(submission.job_id.is_some())),
    ]);
    result.extend(identity_fields);
    let Some(job_id) = submission.job_id else {
        result.insert("state".into(), Value::String("submit-failed".into()));
        result.insert("failure".into(), json!(submission.failure));
        return Ok(Value::Object(result));
    };
    if !watch {
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
    if completed {
        if result.get("resultsDir").is_none() {
            result.insert("state".into(), Value::String("evidence-unavailable".into()));
            result.insert("failure".into(), missing_evidence(&job_id, "artifact_error=null"));
        }
        return Ok(Value::Object(result));
    }
    // The worker ran and failed; a failed preflight names what the host lacks.
    let Some(preflight) = retained
        .manifest
        .as_ref()
        .and_then(|value| value.get("preflight"))
        .filter(|value| value.get("ready").and_then(Value::as_bool) == Some(false))
    else {
        return Ok(Value::Object(result));
    };
    let listed = |key: &str, separator: &str| match preflight.get(key).and_then(Value::as_array) {
        Some(items) => items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(separator),
        None => String::new(),
    };
    // A worker that refused without naming what it lacked is said as such,
    // not given an invented list.
    let missing = match listed("missing", ", ") {
        named if named.is_empty() => "prerequisites the worker did not name".to_string(),
        named => named,
    };
    let remediation = listed("remediation", "; ");
    let detail = match remediation.is_empty() {
        true => format!("missing: {missing}"),
        false => format!("missing: {missing}; remediation: {remediation}"),
    };
    let failure = Failure::config("stado.worker", detail);
    result.insert("preflight".into(), preflight.clone());
    result.insert(
        "failure".into(),
        failure_summary(&failure, format!("Job {job_id} did not execute because the selected host is missing: {missing}.")),
    );
    Ok(Value::Object(result))
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
            repositories.iter().find(|value| {
                value.get("index").and_then(Value::as_u64).is_some_and(|index| NonZeroU64::new(index).is_none())
            })
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

/// The model router's address: HTTPS, or HTTP to this machine's loopback
/// interface, with no credentials, query or fragment. The host is judged by
/// the parsed URL, so a name that merely begins like a loopback address
/// (127.attacker.example) is not taken for one and sent the router token.
pub(crate) fn model_router_url(value: Option<&str>) -> Result<String, Failure> {
    let Some(clean) = value.map(str::trim).filter(|text| !text.is_empty()) else {
        return Err(Failure::config("stado.submit", "STADO_MODEL_ROUTER_URL is required"));
    };
    let parsed = url::Url::parse(clean).map_err(|error| {
        Failure::config("stado.submit", format!("STADO_MODEL_ROUTER_URL {clean} is not a URL: {error}"))
    })?;
    let loopback = match parsed.host() {
        Some(url::Host::Domain(name)) => name == "localhost",
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    };
    let secure = parsed.scheme() == "https";
    if !secure && !(parsed.scheme() == "http" && loopback) {
        return Err(Failure::config(
            "stado.submit",
            "STADO_MODEL_ROUTER_URL must use HTTPS or loopback HTTP",
        ));
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(Failure::config(
            "stado.submit",
            "STADO_MODEL_ROUTER_URL must not contain credentials, query parameters, or a fragment",
        ));
    }
    Ok(clean.trim_end_matches('/').to_string())
}
