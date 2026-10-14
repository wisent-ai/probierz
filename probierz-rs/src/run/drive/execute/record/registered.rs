use crate::run::*;
use serde_json::json;
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
    // A journey belongs to its application and is named by its absolute path,
    // which must arrive whole; a bare name drops the .spec.mjs suffix.
    let filter = opts.spec.as_deref().map(|value| {
        if value.contains('/') {
            value
        } else {
            value.strip_suffix(".spec.mjs").unwrap_or(value)
        }
    });
    let (report, code) =
        crate::specs::execute(name, &artifacts, &report_path, filter, env, Some(run_id))?;
    print_json(&report)?;
    if code == 0 {
        return Ok(());
    }
    Err(Failure::new(
        "run",
        crate::failure::Code::Refused,
        format!("the {name} journey runner ended with status {code}; its report: {report}"),
    )
    .with_exit(code))
}

pub fn run(harness: &Path, name: &str, args: &[String]) -> Answer {
    if target(name).is_none() {
        return Err(fail("cli.run", format!("unknown target: {name}")));
    }
    let opts = parse_run_args(args, false)?;
    // A web journey opens a Weles browser on the machine that runs it, and a
    // browser runs only on a host Stado places it on. `probierz remote run web`
    // is that placement; its job script exports PROBIERZ_SOURCE_IDENTITY, which
    // is how a run here knows it is inside one.
    if name == "web" {
        let placed = std::env::var_os("PROBIERZ_SOURCE_IDENTITY").is_some()
            || opts.env.contains_key("PROBIERZ_SOURCE_IDENTITY");
        if !placed {
            return Err(fail(
                "cli.run",
                "a web journey opens a Weles browser, which runs only on a Stado-placed host: run `probierz remote run web --app <appId> --host <selector>`",
            ));
        }
    }
    if matches!(name, "tui" | "desktop:cua" | "web") {
        return run_registered_surface(harness, name, &opts);
    }
    let mut result = run_surface(
        harness,
        name,
        RunOptions {
            env: opts.env,
            record: opts.record,
            force: opts.force,
            spec: opts.spec,
            app_id: opts.app_id,
            kind: None,
        },
    )?;
    if result.get("skipped").and_then(Value::as_bool) == Some(true) {
        print_json(&result)?;
        // A held resource lock clears by itself; a failed preflight needs
        // the prerequisite its remediation names.
        let code = if result.get("resourceLock").is_some() {
            crate::failure::Code::Unavailable
        } else {
            crate::failure::Code::Prerequisite
        };
        return Err(Failure::new(
            "run",
            code,
            format!("the {name} run was skipped: {result}"),
        ));
    }
    let mut analysis = Value::Null;
    if opts.analyze {
        (analysis, result) = analyze_and_complete(result, opts.frames)?;
    }
    let repair = repair_after_run(harness, &result, opts.no_repair)?;
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
        return Err(Failure::new(
            "run",
            crate::failure::Code::Refused,
            output.to_string(),
        ));
    }
    Ok(())
}

/// The automatic repair of a finished run: none for a run that passed, a run
/// an authoring session is staging, a caller that passed `--no-repair`, or
/// while `PROBIERZ_REPAIR_SUPPRESS` is set. A failed run that recorded no
/// `appId` answers that refusal rather than repairing another application.
pub(crate) fn repair_after_run(
    harness: &Path,
    completed: &Value,
    no_repair: bool,
) -> Result<Value, Failure> {
    let passed = completed.get("passed").and_then(Value::as_bool) == Some(true);
    let authoring = completed
        .get("spec")
        .and_then(Value::as_str)
        .is_some_and(|spec| spec.contains(".author-staging-"));
    if passed || authoring || no_repair || std::env::var_os("PROBIERZ_REPAIR_SUPPRESS").is_some() {
        return Ok(Value::Null);
    }
    let Some(app_id) = completed.get("appId").and_then(Value::as_str) else {
        return Ok(json!({
            "error": "the run recorded no appId, so no application's repair was attempted"
        }));
    };
    crate::authoring::repair_failed_run(
        harness,
        app_id,
        completed.get("runId").and_then(Value::as_str),
        1,
        false,
    )
}
