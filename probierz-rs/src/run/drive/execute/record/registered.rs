use serde_json::json;
use crate::run::*;
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

