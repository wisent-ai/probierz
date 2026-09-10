use serde_json::json;
use crate::specs::*;

/// Run one surface's journeys and write the canonical report.
///
/// The exit code is the operator's answer: zero when every journey passed.
pub fn execute(
    surface: &str,
    harness: &Path,
    artifacts: &Path,
    report_path: &Path,
    filter: Option<&str>,
    env: BTreeMap<String, String>,
    run_id: Option<String>,
) -> Result<(Value, i32), Failure> {
    let specs = select(surface, filter);
    // A filter that matches nothing registered may still name a journey the
    // application owns in its own repository. That is a declaration, not a
    // mistake, so it is resolved before anything is refused.
    let external = if specs.is_empty() {
        match filter {
            Some(want) => Some(External::resolve(want)?),
            None => {
                return Err(fail(
                    "specs.select",
                    format!("no journey is registered for surface {surface}"),
                ))
            }
        }
    } else {
        None
    };
    fs::create_dir_all(artifacts).map_err(|error| {
        fail(
            "specs.artifacts",
            format!("{}: {error}", artifacts.display()),
        )
    })?;

    let mut capture_errors: Vec<String> = Vec::new();
    let mut rows: Vec<Value> = Vec::new();
    for spec in &specs {
        let context = Context {
            harness: harness.to_path_buf(),
            artifacts: artifacts.to_path_buf(),
            title: spec.title.to_string(),
            env: env.clone(),
            media: Mutex::new(Vec::new()),
        };
        let started_at = SystemTime::now();
        let started = Instant::now();
        let outcome = catch_unwind(AssertUnwindSafe(|| (spec.run)(&context)));
        let duration = started.elapsed();
        let error = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(reason)) => Some(reason),
            Err(panic) => Some(format!("journey panicked: {}", panic_text(panic))),
        };
        let media = match validate_media(artifacts, &context.declared_media()) {
            Ok(media) => media,
            Err(reason) => {
                capture_errors.push(format!("{}: {reason}", spec.title));
                Vec::new()
            }
        };
        rows.push(json!({
            "title": spec.title,
            "passed": error.is_none(),
            "status": if error.is_none() { "passed" } else { "failed" },
            "flaky": false,
            "attempts": 1,
            "duration": duration.as_millis(),
            "startedAt": iso_timestamp(started_at),
            "completedAt": at_iso(started_at, duration),
            "error": error.as_deref().map(clip_row_error).map(Value::from).unwrap_or(Value::Null),
            "media": media,
        }));
    }

    if let Some(external) = &external {
        let started_at = SystemTime::now();
        let started = Instant::now();
        let error = external.run(artifacts, &env).err();
        let duration = started.elapsed();
        rows.push(json!({
            "title": external.title,
            "passed": error.is_none(),
            "status": if error.is_none() { "passed" } else { "failed" },
            "flaky": false,
            "attempts": 1,
            "duration": duration.as_millis(),
            "startedAt": iso_timestamp(started_at),
            "completedAt": at_iso(started_at, duration),
            "error": error.as_deref().map(clip_row_error).map(Value::from).unwrap_or(Value::Null),
            "media": Vec::<Media>::new(),
            "owner": "application",
        }));
    }

    let passed = rows.iter().filter(|row| row["status"] == "passed").count();
    let report = json!({
        "probierz": { "runId": run_id, "captureErrors": capture_errors },
        "total": rows.len(),
        "passed": passed,
        "failed": rows.len() - passed,
        "flaky": 0,
        "skipped": 0,
        "tests": rows,
    });
    write_report(report_path, &report)?;
    let code = if passed == rows.len() { 0 } else { 1 };
    Ok((report, code))
}

