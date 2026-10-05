use crate::specs::*;
use serde_json::json;

/// Run the application-owned journey `filter` names on one surface and write
/// the canonical report.
///
/// Probierz carries no journeys of its own: every journey lives in the
/// product's own tree, approved by the operator there, so a run names it.
/// The exit code is the operator's answer: zero when the journey passed.
pub fn execute(
    surface: &str,
    artifacts: &Path,
    report_path: &Path,
    filter: Option<&str>,
    env: BTreeMap<String, String>,
    run_id: Option<String>,
) -> Result<(Value, i32), Failure> {
    let Some(want) = filter else {
        return Err(fail(
            "specs.select",
            format!(
                "surface {surface} runs only a journey its application owns; name it with --filter"
            ),
        ));
    };
    let external = External::resolve(want)?;
    fs::create_dir_all(artifacts).map_err(|error| {
        fail(
            "specs.artifacts",
            format!("{}: {error}", artifacts.display()),
        )
    })?;

    let started_at = SystemTime::now();
    let started = Instant::now();
    let error = external.run(artifacts, &env).err();
    let duration = started.elapsed();
    let passed = error.is_none();
    let row = json!({
        "title": external.title,
        "passed": passed,
        "status": if passed { "passed" } else { "failed" },
        "flaky": false,
        "attempts": 1,
        "duration": duration.as_millis(),
        "startedAt": iso_timestamp(started_at),
        "completedAt": at_iso(started_at, duration),
        // The row's error whole: the expectation and what the application showed both stay.
        "error": error.as_deref().map(Value::from).unwrap_or(Value::Null),
        "media": [],
        "owner": "application",
    });
    let report = json!({
        "probierz": { "runId": run_id, "captureErrors": [] },
        "total": 1,
        "passed": if passed { 1 } else { 0 },
        "failed": if passed { 0 } else { 1 },
        "flaky": 0,
        "skipped": 0,
        "tests": [row],
    });
    write_report(report_path, &report)?;
    Ok((report, if passed { 0 } else { 1 }))
}
