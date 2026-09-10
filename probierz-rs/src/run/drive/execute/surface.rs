use serde_json::json;
use crate::run::*;
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
    let (app, surface) = declared_surface(harness, &app_id, name, &mut opts)?;
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
    let spec = if byk {
        Some("byk-auth.e2e.ts".to_string())
    } else {
        configured_spec
    };
    let mut env = suite_environment(
        harness,
        &app_id,
        &run_id,
        &artifacts,
        &report_path,
        &journeys,
        record,
        spec.as_deref(),
        byk,
        &opts,
    );
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

