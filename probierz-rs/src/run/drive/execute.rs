use serde_json::json;
use crate::run::*;
pub(crate) struct RunOptions {
    pub(crate) env: BTreeMap<String, String>,
    /// Run the byk-auth suite on this machine instead of the dedicated host.
    pub(crate) local: bool,
    /// The fleet host the remote byk-auth suite is placed on.
    pub(crate) host_selector: String,
    pub(crate) record: bool,
    pub(crate) timeout_ms: u64,
    pub(crate) force: bool,
    pub(crate) spec: Option<String>,
    pub(crate) app_id: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) resource_wait_ms: Option<u64>,
}

pub(crate) fn drain_run_stream<R: Read>(
    mut stream: R,
    path: &Path,
    secrets: &[(String, String)],
    run_started: DateTime<Utc>,
) -> (String, Option<u64>) {
    let mut tail = String::new();
    let mut first_output_ms = None;
    let mut buffer = [0u8; 8192];
    loop {
        let count = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        if first_output_ms.is_none() {
            first_output_ms = Some(
                (Utc::now().timestamp_millis() - run_started.timestamp_millis()).max(0) as u64,
            );
        }
        let safe = redact_text(&String::from_utf8_lossy(&buffer[..count]), secrets);
        tail = tail_chars(&(tail + &safe), TAIL);
        let _ = append_secure(path, stamped(&safe).as_bytes());
    }
    (tail, first_output_ms)
}

pub(crate) fn execute_suite(
    // Only `mobile:ios:byk-auth` reads these: run its suite here rather than on
    // the fleet, and which fleet host to place it on when it is remote.
    local: bool,
    host_selector: &str,
    harness: &Path,
    script: &str,
    env: &BTreeMap<String, String>,
    timeout_ms: u64,
    secrets: Vec<(String, String)>,
    stdout_path: &Path,
    stderr_path: &Path,
    target_name: &str,
    started_at: &str,
    artifacts: &Path,
) -> Result<(i32, bool, String, String, Value, Value), Failure> {
    if target_name == "mobile:ios:byk-auth" {
        return execute_byk(
            local,
            host_selector,
            harness,
            env,
            timeout_ms,
            &secrets,
            stdout_path,
            stderr_path,
            started_at,
            artifacts,
        );
    }
    let mut command = Command::new("npm");
    command
        .args(["run", script])
        .current_dir(harness)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        Failure::config(
            "run.spawn",
            format!("Starting the {target_name} runner failed: {error}"),
        )
    })?;
    let pid = child.id();
    let child_out = child.stdout.take().expect("piped stdout");
    let child_err = child.stderr.take().expect("piped stderr");
    let out_path = stdout_path.to_path_buf();
    let err_path = stderr_path.to_path_buf();
    let out_secrets = secrets.clone();
    let err_secrets = secrets;
    let run_started = DateTime::parse_from_rfc3339(started_at)
        .map(|date| date.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let out_thread =
        thread::spawn(move || drain_run_stream(child_out, &out_path, &out_secrets, run_started));
    let err_thread =
        thread::spawn(move || drain_run_stream(child_err, &err_path, &err_secrets, run_started));

    let started = Instant::now();
    let process_name = {
        let app_path = if matches!(target_name, "desktop:mac" | "desktop:cua") {
            env.get("MAC_APP_PATH")
        } else {
            env.get("APP_IOS")
        };
        app_path
            .and_then(|path| Path::new(path).file_stem())
            .and_then(|name| name.to_str())
            .map(str::to_string)
    };
    let mut samples = Vec::new();
    if let Some(sample) = performance_sample(pid, process_name.as_deref()) {
        samples.push(sample);
    }
    let mut next_sample = Instant::now() + Duration::from_millis(SAMPLE_INTERVAL_MS);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= Duration::from_millis(timeout_ms) {
                    timed_out = true;
                    terminate_tree(&mut child, false);
                    thread::sleep(Duration::from_millis(25));
                    if child.try_wait().ok().flatten().is_none() {
                        terminate_tree(&mut child, true);
                    }
                    break child.wait()?;
                }
                if Instant::now() >= next_sample {
                    if let Some(sample) = performance_sample(pid, process_name.as_deref()) {
                        samples.push(sample);
                    }
                    next_sample = Instant::now() + Duration::from_millis(SAMPLE_INTERVAL_MS);
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(Failure::unavailable("run.wait", error.to_string())),
        }
    };
    if let Some(sample) = performance_sample(pid, process_name.as_deref()) {
        samples.push(sample);
    }
    let (safe_out, first_out) = out_thread.join().unwrap_or_default();
    let (safe_err, first_err) = err_thread.join().unwrap_or_default();
    let first_output_ms = match (first_out, first_err) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    };
    let rss: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.get("rssKb").and_then(Value::as_f64))
        .collect();
    let cpu: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.get("cpuPercent").and_then(Value::as_f64))
        .collect();
    let app_rss: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.pointer("/app/rssKb").and_then(Value::as_f64))
        .collect();
    let app_cpu: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.pointer("/app/cpuPercent").and_then(Value::as_f64))
        .collect();
    let average = |values: &[f64]| {
        if values.is_empty() {
            Value::Null
        } else {
            number(values.iter().sum::<f64>() / values.len() as f64)
        }
    };
    let performance = json!({
        "schemaVersion": 1,
        "subject": "run-and-app-processes",
        "firstOutputMs": first_output_ms,
        "intervalMs": SAMPLE_INTERVAL_MS,
        "peakRssKb": rss.iter().copied().max_by(f64::total_cmp).map(number).unwrap_or(Value::Null),
        "averageCpuPercent": average(&cpu),
        "appProcessName": process_name,
        "appPeakRssKb": app_rss.iter().copied().max_by(f64::total_cmp).map(number).unwrap_or(Value::Null),
        "appAverageCpuPercent": average(&app_cpu),
        "samples": samples,
    });
    let performance_path = artifacts.join("performance.json");
    write_json(&performance_path, &performance)?;
    let mut public = performance.clone();
    public
        .as_object_mut()
        .expect("object")
        .insert("file".into(), json!(performance_path));
    public
        .as_object_mut()
        .expect("object")
        .shift_remove("samples");
    let diagnostics = collect_platform_diagnostics(target_name, env, artifacts, started_at);
    Ok((
        status.code().unwrap_or(-1),
        timed_out,
        safe_out,
        safe_err,
        public,
        diagnostics,
    ))
}

pub(crate) fn run_surface(harness: &Path, name: &str, mut opts: RunOptions) -> Result<Value, Failure> {
    let config = target(name).ok_or_else(|| {
        fail(
            "run.target",
            format!(
                "unknown target: {name} (one of {})",
                target_list().join(", ")
            ),
        )
    })?;
    let app_id = segment(
        opts.app_id
            .as_deref()
            .or_else(|| opts.env.get("PROBIERZ_APP_ID").map(String::as_str)),
        "probierz",
    );
    let mut app = None;
    let mut surface = None;
    if app_id != "probierz" {
        let (declaration, value) = app_surface(harness, &app_id, name)?;
        let mut conditions = yaml_map_strings(value.get("conditions"));
        conditions.extend(opts.env.clone());
        opts.env = conditions;
        for secret in declaration
            .document
            .get("secretRefs")
            .and_then(serde_yaml::Value::as_mapping)
            .into_iter()
            .flatten()
            .filter_map(|(key, _)| key.as_str())
        {
            if !opts.env.contains_key(secret) {
                if let Ok(value) = std::env::var(secret) {
                    opts.env.insert(secret.into(), value);
                }
            }
        }
        for (target_name, source_name) in value
            .get("env")
            .and_then(serde_yaml::Value::as_mapping)
            .into_iter()
            .flatten()
            .filter_map(|(key, value)| Some((key.as_str()?, value.as_str()?)))
        {
            if let Some(value) = opts
                .env
                .get(source_name)
                .cloned()
                .or_else(|| std::env::var(source_name).ok())
            {
                opts.env.insert(source_name.into(), value.clone());
                opts.env.insert(target_name.into(), value);
            }
        }
        surface = Some(value);
        app = Some(declaration);
    }
    let configured_spec = opts.spec.clone().or_else(|| {
        surface
            .as_ref()
            .and_then(|value| value.get("spec"))
            .and_then(serde_yaml::Value::as_str)
            .map(str::to_string)
    });
    let byk = name == "mobile:ios:byk-auth";
    let record = if byk { false } else { opts.record };
    if byk {
        let allowed = [
            "APP_IOS",
            "BUNDLE_ID",
            "IOS_DEVICE",
            "IOS_VERSION",
            "APPIUM_HOME",
            "DEVELOPER_DIR",
        ];
        if opts
            .env
            .keys()
            .any(|name| !allowed.contains(&name.as_str()))
        {
            return Err(fail("run.conditions","mobile:ios:byk-auth accepts only app, device, runtime, Appium, and Xcode path conditions"));
        }
    }
    let started_date = Utc::now();
    let started_time = SystemTime::now();
    let started_at = started_date.to_rfc3339_opts(SecondsFormat::Millis, true);
    let run_id = unique_run_id(started_date);
    let artifacts = harness
        .join("test-results")
        .join(&app_id)
        .join(segment(Some(name), "target"))
        .join(&started_at[..10])
        .join(&run_id);
    for child in ["media", "frames", "diagnostics"] {
        fs::create_dir_all(artifacts.join(child))?;
    }
    let report_path = artifacts.join("report.json");
    let manifest_path = artifacts.join("run-manifest.json");
    let stdout_path = artifacts.join("stdout.log");
    let stderr_path = artifacts.join("stderr.log");
    let build = build_identity(harness, &opts.env)?;
    let kind = segment(
        opts.kind
            .as_deref()
            .or_else(|| opts.env.get("PROBIERZ_RUN_KIND").map(String::as_str)),
        "adhoc",
    );
    let journeys = surface
        .as_ref()
        .map(|value| manifest::surface_journeys(value, &opts.env))
        .unwrap_or_default();
    let submitted = submitted_source_identity(Some(&app_id))?;
    let (source, harness_identity, origin) = if let Some(submitted) = submitted {
        (
            submitted.pointer("/app").cloned().unwrap_or(Value::Null),
            submitted
                .pointer("/harness")
                .cloned()
                .unwrap_or(Value::Null),
            "submitter",
        )
    } else {
        let source = if app.is_some() {
            app_source_identity(harness, &app_id)?
                .get("app")
                .cloned()
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        (
            source,
            repository_identity(harness, "probierz", None, true, true)?,
            "runner",
        )
    };
    let conditions = run_conditions(record, &opts.env);
    let mut base = json!({"runId":run_id,"startedAt":started_at,"appId":app_id,"kind":kind,"target":name,"tool":config.tool,"pkg":config.pkg,"script":config.script,"artifactsDir":artifacts,"reportPath":report_path,"manifestPath":manifest_path,"stdoutPath":stdout_path,"stderrPath":stderr_path,"conditions":conditions});
    let app_manifest=app.as_ref().map(|declaration|json!({"file":declaration.file,"owner":declaration.document.get("owner").and_then(serde_yaml::Value::as_str).unwrap_or(""),"journeys":journeys})).unwrap_or(Value::Null);
    let host_name = capture_text("hostname", &[], None, Some(3000));
    let release = capture_text("uname", &["-r"], None, Some(3000));
    let node = capture_text("node", &["--version"], None, Some(3000));
    write_json(
        &manifest_path,
        &json!({"schemaVersion":2,"runId":run_id,"appId":app_id,"kind":kind,"target":name,"spec":if byk{json!("byk-auth.e2e.ts")}else{configured_spec.clone().map(Value::String).unwrap_or(Value::Null)},"status":"preflight","startedAt":started_at,"harness":harness_identity,"source":source,"sourceIdentityOrigin":origin,"build":build,"appVersion":opts.env.get("PROBIERZ_APP_VERSION"),"appManifest":app_manifest,"host":{"hostname":text(&host_name.stdout).trim(),"platform":match std::env::consts::OS{"macos"=>"darwin","windows"=>"win32",other=>other},"release":text(&release.stdout).trim(),"arch":node_arch(),"node":text(&node.stdout).trim()},"device":{"name":opts.env.get("IOS_DEVICE").or_else(||opts.env.get("ANDROID_DEVICE")),"runtime":opts.env.get("IOS_VERSION").or_else(||opts.env.get("ANDROID_VERSION"))},"conditions":conditions,"paths":{"artifactsDir":artifacts,"reportPath":report_path,"stdoutPath":stdout_path,"stderrPath":stderr_path}}),
    )?;
    if !opts.force {
        let pf = preflight(harness, if byk { "mobile:ios" } else { name }, &opts.env)?;
        if !pf.get("ready").and_then(Value::as_bool).unwrap_or(false) {
            update_json(
                &manifest_path,
                &json!({"status":"blocked","completedAt":now_iso(),"preflight":pf,"artifacts":artifact_hashes(&artifacts,&manifest_path)?}),
            )?;
            let mut result = base.as_object().expect("object").clone();
            result.extend(
                json!({"ready":false,"skipped":true,"preflight":pf})
                    .as_object()
                    .expect("object")
                    .clone(),
            );
            return Ok(Value::Object(result));
        }
    }
    let mut env = env_snapshot(&opts.env);
    env.insert("PROBIERZ_APP_ID".into(), app_id.clone());
    env.insert("PROBIERZ_RUN_ID".into(), run_id.clone());
    env.insert(
        "PROBIERZ_TOOLKIT_ROOT".into(),
        harness.to_string_lossy().into_owned(),
    );
    env.insert(
        "PROBIERZ_ARTIFACTS".into(),
        artifacts.to_string_lossy().into_owned(),
    );
    env.insert(
        "PROBIERZ_REPORT_PATH".into(),
        report_path.to_string_lossy().into_owned(),
    );
    env.insert("PROBIERZ_JOURNEYS".into(), journeys.join(","));
    env.insert(
        "PROBIERZ_NATIVE_CAPTURE_BIN".into(),
        harness
            .join("node_modules/.cache/probierz/screen-capture-kit")
            .to_string_lossy()
            .into_owned(),
    );
    if record {
        env.insert("PROBIERZ_RECORD".into(), "1".into());
    }
    let spec = if byk {
        Some("byk-auth.e2e.ts".into())
    } else {
        configured_spec
    };
    if let Some(spec) = &spec {
        if !byk {
            env.insert("PROBIERZ_SPEC".into(), spec.clone());
        }
    }
    if let Ok(binary) = std::env::current_exe() {
        env.insert("PROBIERZ_BIN".into(), binary.to_string_lossy().into_owned());
    }
    let resources = crate::evidence::resources_for(name, &opts.env);
    let lease = match crate::evidence::acquire_resources_wait(
        harness,
        &resources,
        &run_id,
        opts.resource_wait_ms,
    ) {
        Ok(lease) => lease,
        Err(error) => {
            let lock = json!({"error":error.to_string(),"resource":null,"owner":null});
            update_json(
                &manifest_path,
                &json!({"status":"blocked","completedAt":now_iso(),"resourceLock":lock,"artifacts":artifact_hashes(&artifacts,&manifest_path)?}),
            )?;
            let mut result = base.as_object().expect("object").clone();
            result.extend(
                json!({"ready":false,"skipped":true,"resourceLock":lock})
                    .as_object()
                    .expect("object")
                    .clone(),
            );
            return Ok(Value::Object(result));
        }
    };
    update_json(&manifest_path, &json!({"resources":lease.resources}))?;
    let lifecycle = app
        .as_ref()
        .and_then(|declaration| declaration.document.get("data"));
    let secrets = secret_values(&opts.env);
    let seed = run_data_command(
        harness,
        lifecycle.and_then(|value| value.get("seed")),
        &env,
        &secrets,
        &stdout_path,
        &stderr_path,
    );
    if seed.get("ok").and_then(Value::as_bool) == Some(true) {
        if let Some(seed_env) = seed.pointer("/result/env").and_then(Value::as_object) {
            let mut seeded_values = BTreeMap::new();
            for (name, value) in seed_env {
                let value = value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string());
                seeded_values.insert(name.clone(), value.clone());
                opts.env.insert(name.clone(), value.clone());
                env.insert(name.clone(), value);
            }
            let updated_conditions = run_conditions(record, &opts.env);
            base.as_object_mut()
                .expect("object")
                .insert("conditions".into(), updated_conditions.clone());
            let mut public_seed = seed.get("result").cloned().unwrap_or_else(|| json!({}));
            public_seed
                .as_object_mut()
                .expect("seed object")
                .insert("env".into(), redacted_environment(&seeded_values));
            update_json(
                &manifest_path,
                &json!({ "conditions": updated_conditions, "seed": public_seed }),
            )?;
        }
    }
    if seed.get("ok").and_then(Value::as_bool) != Some(true) {
        let cleanup = run_data_command(
            harness,
            lifecycle.and_then(|value| value.get("cleanup")),
            &env,
            &secrets,
            &stdout_path,
            &stderr_path,
        );
        let error = seed.get("error").and_then(Value::as_str).unwrap_or("");
        let validation = json!({"ok":false,"error":format!("seed failed: {error}")});
        let mut result = base.as_object().expect("object").clone();
        result.extend(json!({"ready":true,"skipped":false,"command":null,"spec":spec,"exitCode":1,"signal":null,"timedOut":false,"passed":false,"durationMs":Utc::now().timestamp_millis()-started_date.timestamp_millis(),"reportValidation":validation,"setupError":error,"cleanup":cleanup,"stdoutTail":"","stderrTail":error}).as_object().expect("object").clone());
        update_json(
            &manifest_path,
            &json!({"status":"failed","completedAt":now_iso(),"setupError":error,"cleanup":cleanup,"reportValidation":validation}),
        )?;
        return Ok(Value::Object(result));
    }
    let data_seeded = lifecycle.and_then(|value| value.get("seed")).is_some();
    let command_text = format!(
        "npm run {}{}",
        config.script,
        spec.as_ref()
            .filter(|_| !byk)
            .map(|spec| format!(" (PROBIERZ_SPEC={spec})"))
            .unwrap_or_default()
    );
    let timeout = if opts.timeout_ms > 0 {
        opts.timeout_ms
    } else {
        DEFAULT_TIMEOUT_MS
    };
    update_json(
        &manifest_path,
        &json!({"status":"running","command":command_text,"timeoutMs":timeout,"dataSeeded":data_seeded}),
    )?;
    let (exit_code, timed_out, out, err, performance, platform) = execute_suite(
        opts.local,
        &opts.host_selector,
        harness,
        config.script,
        &env,
        timeout,
        secret_values(&opts.env),
        &stdout_path,
        &stderr_path,
        name,
        &started_at,
        &artifacts,
    )?;
    let validation = report_identity(&report_path, &run_id, started_time);
    let cleanup = if data_seeded {
        run_data_command(
            harness,
            lifecycle.and_then(|value| value.get("cleanup")),
            &env,
            &secret_values(&opts.env),
            &stdout_path,
            &stderr_path,
        )
    } else {
        json!({"ok":true,"result":null})
    };
    let passed = exit_code == 0
        && !timed_out
        && validation.get("ok").and_then(Value::as_bool) == Some(true)
        && cleanup.get("ok").and_then(Value::as_bool) == Some(true);
    let mut result = base.as_object().expect("object").clone();
    result.extend(json!({"ready":true,"command":command_text,"spec":spec,"exitCode":exit_code,"signal":null,"timedOut":timed_out,"canceled":false,"passed":passed,"durationMs":Utc::now().timestamp_millis()-started_date.timestamp_millis(),"reportValidation":validation,"stdoutTail":out,"stderrTail":err,"cleanup":cleanup,"cleanupError":if cleanup.get("ok").and_then(Value::as_bool)==Some(true){Value::Null}else{cleanup.get("error").cloned().unwrap_or(Value::Null)},"performance":performance,"platformDiagnostics":platform}).as_object().expect("object").clone());
    update_json(
        &manifest_path,
        &json!({"status":if passed{"executed"}else{"failed"},"completedAt":now_iso(),"exitCode":exit_code,"signal":null,"timedOut":timed_out,"canceled":false,"durationMs":result.get("durationMs"),"reportValidation":validation,"cleanup":cleanup,"cleanupError":result.get("cleanupError"),"performance":performance,"platformDiagnostics":platform,"artifacts":artifact_hashes(&artifacts,&manifest_path)?}),
    )?;
    Ok(Value::Object(result))
}

pub(crate) fn complete_run(
    mut run: Value,
    analysis: Option<&Value>,
    analysis_error: Option<&str>,
) -> Result<Value, Failure> {
    if run.get("canceled").and_then(Value::as_bool) == Some(true) {
        return Ok(run);
    }
    let artifacts = PathBuf::from(
        run.get("artifactsDir")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let manifest_path = PathBuf::from(
        run.get("manifestPath")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let run_id = run
        .get("runId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let analysis_path = artifacts.join("analysis.json");
    let payload = if let Some(error) = analysis_error {
        json!({"runId":run_id,"error":error})
    } else {
        let mut value = analysis.cloned().unwrap_or_else(|| json!({}));
        value
            .as_object_mut()
            .expect("object")
            .insert("runId".into(), json!(run_id));
        value
    };
    write_json(&analysis_path, &payload)?;
    let capture_errors = analysis
        .and_then(|value| value.get("captureErrors"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let media = analysis
        .and_then(|value| value.get("media"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let missing: Vec<Value> = media
        .iter()
        .filter(|item| item.get("missing").and_then(Value::as_bool) == Some(true))
        .cloned()
        .collect();
    let crashes = analysis
        .and_then(|value| value.pointer("/diagnostics/crashes"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let valid = analysis_error.is_none()
        && analysis.is_some_and(|value| {
            value.get("runId").and_then(Value::as_str) == Some(&run_id)
                && js_number(value.get("total")) > 0.0
                && js_number(value.get("failed")) == 0.0
        })
        && capture_errors.is_empty()
        && missing.is_empty()
        && crashes.is_empty();
    let kinds: BTreeSet<&str> = media
        .iter()
        .filter_map(|item| item.get("kind").and_then(Value::as_str))
        .collect();
    let required = run
        .pointer("/conditions/record")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let present = !required
        || ["video", "trace", "screenshot"]
            .iter()
            .any(|kind| kinds.contains(kind));
    let mut errors: Vec<Value> = analysis_error
        .map(|error| vec![json!(error)])
        .unwrap_or_default();
    if analysis_error.is_none()
        && analysis
            .and_then(|value| value.get("runId"))
            .and_then(Value::as_str)
            != Some(&run_id)
    {
        errors.push(json!("analysis run ID mismatch"));
    }
    if analysis_error.is_none()
        && analysis
            .map(|value| js_number(value.get("total")) <= 0.0)
            .unwrap_or(true)
    {
        errors.push(json!("zero executed checks"));
    }
    if analysis_error.is_none()
        && analysis
            .map(|value| js_number(value.get("failed")) > 0.0)
            .unwrap_or(false)
    {
        errors.push(json!(format!(
            "{} failed checks",
            analysis
                .map(|value| js_number(value.get("failed")))
                .unwrap_or(0.0)
        )));
    }
    errors.extend(capture_errors.clone());
    errors.extend(missing.iter().map(|item| {
        json!(format!(
            "missing report-typed artifact: {}",
            item.get("file").and_then(Value::as_str).unwrap_or("")
        ))
    }));
    if !present {
        errors.push(json!(
            "recording requested but no report-typed capture was produced"
        ));
    }
    errors.extend(crashes.iter().map(|item| {
        json!(format!(
            "crash evidence: {}",
            item.get("message")
                .or_else(|| item.get("source"))
                .and_then(Value::as_str)
                .unwrap_or("unknown crash")
        ))
    }));
    let evidence = json!({"report":run.pointer("/reportValidation/ok").and_then(Value::as_bool).unwrap_or(false),"analysis":valid,"captureRequired":required,"capturePresent":present,"captureErrors":capture_errors,"missingMedia":missing.iter().filter_map(|item|item.get("file").cloned()).collect::<Vec<_>>(),"crashes":crashes,"errors":errors});
    let passed = run.get("passed").and_then(Value::as_bool).unwrap_or(false)
        && evidence.get("report").and_then(Value::as_bool) == Some(true)
        && valid
        && present;
    update_json(
        &manifest_path,
        &json!({"status":if passed{"passed"}else{"failed"},"completedAt":now_iso(),"exitCode":run.get("exitCode"),"timedOut":run.get("timedOut"),"reportValidation":run.get("reportValidation"),"evidence":evidence,"failure":Value::Null,"analysisPath":analysis_path,"artifacts":artifact_hashes(&artifacts,&manifest_path)?}),
    )?;
    let object = run.as_object_mut().expect("object");
    object.insert("passed".into(), json!(passed));
    object.insert("analysisPath".into(), json!(analysis_path));
    object.insert("evidence".into(), evidence);
    Ok(run)
}

pub(crate) fn run_registered_surface(harness: &Path, name: &str, opts: &RunArgs) -> Answer {
    let mut env = env_snapshot(&opts.env);
    let run_id = format!(
        "rust-{}-{}",
        name.replace(':', "-"),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let artifacts = opts
        .env
        .get("PROBIERZ_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| harness.join("test-results").join(&run_id));
    let report_path = artifacts.join("report.json");
    env.insert("PROBIERZ_RUN_ID".into(), run_id.clone());
    env.insert(
        "PROBIERZ_ARTIFACTS".into(),
        artifacts.to_string_lossy().into_owned(),
    );
    env.insert(
        "PROBIERZ_REPORT_PATH".into(),
        report_path.to_string_lossy().into_owned(),
    );
    // A registered journey is named by its title; an application-owned one is
    // named by its absolute path, and a path must arrive whole.
    let filter = opts.spec.as_deref().map(|value| {
        if value.contains('/') {
            value
        } else {
            value.strip_suffix(".spec.mjs").unwrap_or(value)
        }
    });
    let (report, code) = crate::specs::execute(
        name,
        harness,
        &artifacts,
        &report_path,
        filter,
        env,
        Some(run_id),
    )?;
    print_json(&report)?;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

pub fn run(harness: &Path, name: &str, args: &[String]) -> Answer {
    if target(name).is_none() {
        return Err(fail("cli.run", format!("unknown target: {name}")));
    }
    let opts = parse_run_args(args, false)?;
    // A flag that belongs to one target is refused before anything executes:
    // running a journey while silently ignoring what the operator asked for is
    // worse than refusing.
    if (opts.local || opts.seed_resend) && name != "mobile:ios:byk-auth" {
        return Err(fail(
            "cli.run",
            format!("--local and --seed-resend apply to mobile:ios:byk-auth, not {name}"),
        ));
    }
    if matches!(name, "tui" | "desktop:cua") {
        return run_registered_surface(harness, name, &opts);
    }
    if opts.seed_resend {
        return seed_byk_resend(harness, &opts.env);
    }
    let mut result = run_surface(
        harness,
        name,
        RunOptions {
            host_selector: byk_host_selector(opts.host.as_deref(), &opts.env),
            env: opts.env,
            local: opts.local,
            record: opts.record,
            timeout_ms: opts.timeout_ms,
            force: opts.force,
            spec: opts.spec,
            app_id: opts.app_id,
            kind: None,
            resource_wait_ms: opts.resource_wait_ms,
        },
    )?;
    if result.get("skipped").and_then(Value::as_bool) == Some(true) {
        print_json(&result)?;
        std::process::exit(3);
    }
    let mut analysis = Value::Null;
    if opts.analyze {
        match analyze_run(
            Path::new(
                result
                    .get("reportPath")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
            Some(Path::new(
                result
                    .get("artifactsDir")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )),
            result.get("tool").and_then(Value::as_str),
            opts.frames,
            result.get("runId").and_then(Value::as_str),
        ) {
            Ok(value) => {
                analysis = value;
                result = complete_run(result, Some(&analysis), None)?;
            }
            Err(error) => {
                analysis = json!({"error": error.detail});
                result = complete_run(result, None, Some(&error.detail))?;
            }
        }
    }
    let authoring = result
        .get("spec")
        .and_then(Value::as_str)
        .unwrap_or("")
        .contains(".author-staging-");
    let repair = if !result
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && !authoring
        && !opts.no_repair
        && std::env::var_os("PROBIERZ_REPAIR_SUPPRESS").is_none()
    {
        crate::authoring::repair_failed_run(
            harness,
            result
                .get("appId")
                .and_then(Value::as_str)
                .unwrap_or("probierz"),
            result.get("runId").and_then(Value::as_str),
            1,
            false,
        )?
    } else {
        Value::Null
    };
    let mut output = result.clone();
    output
        .as_object_mut()
        .expect("object")
        .insert("analysis".into(), analysis);
    output
        .as_object_mut()
        .expect("object")
        .insert("repair".into(), repair);
    let passed = result
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    print_json(&output)?;
    if !passed {
        std::process::exit(1);
    }
    Ok(())
}

