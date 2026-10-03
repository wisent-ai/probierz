//! Recorded runs: one owner-only JSON file per run, never rewritten, under
//! `test-results/.benchmark/<app>/`.

use std::path::{Path, PathBuf};

use serde_json::Value as Json;
use sha2::{Digest, Sha256};

use crate::benchmark::RUN_SCHEMA;
use crate::failure::{create_private, Code, Failure};

fn directory(harness: &Path, app_id: &str) -> PathBuf {
    harness.join("test-results").join(".benchmark").join(app_id)
}

/// A run's name: when it started and what it holds.
pub(crate) fn identity(started_at: &str, body: &Json) -> String {
    let digest = Sha256::digest(format!("{started_at}{body}").as_bytes());
    let stamp: String = started_at
        .chars()
        .filter(|character| character.is_ascii_digit())
        .take(14)
        .collect();
    format!("bench-{stamp}-{}", &hex::encode(digest)[..8])
}

pub(crate) fn write(
    harness: &Path,
    app_id: &str,
    run_id: &str,
    run: &Json,
) -> Result<PathBuf, Failure> {
    let directory = directory(harness, app_id);
    std::fs::create_dir_all(&directory)?;
    let file = directory.join(format!("{run_id}.json"));
    if file.exists() {
        return Err(Failure::new(
            "benchmark.store",
            Code::Invalid,
            format!(
                "run {run_id} is already recorded at {}; a run is never rewritten",
                file.display()
            ),
        ));
    }
    let mut handle = create_private(&file)?;
    std::io::Write::write_all(&mut handle, serde_json::to_string_pretty(run)?.as_bytes())?;
    Ok(file)
}

fn valid_run_id(run_id: &str) -> bool {
    run_id.starts_with("bench-")
        && run_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
}

pub(crate) fn read(harness: &Path, app_id: &str, run_id: &str) -> Result<Json, Failure> {
    if !valid_run_id(run_id) {
        return Err(Failure::invalid(
            "benchmark.run-id",
            format!(
                "{run_id} is not a benchmark run id; run ids look like bench-20261003172236-1a2b3c4d and probierz benchmark list {app_id} lists them"
            ),
        ));
    }
    let file = directory(harness, app_id).join(format!("{run_id}.json"));
    let bytes = std::fs::read(&file).map_err(|error| {
        Failure::invalid(
            "benchmark.run-id",
            format!(
                "no run {run_id} is recorded for {app_id} at {} ({error}); probierz benchmark list {app_id} lists the recorded runs",
                file.display()
            ),
        )
    })?;
    let run: Json = serde_json::from_slice(&bytes).map_err(|error| {
        Failure::config(
            "benchmark.store",
            format!("{} is not JSON: {error}", file.display()),
        )
    })?;
    if run["schema"] != RUN_SCHEMA {
        return Err(Failure::config(
            "benchmark.store",
            format!(
                "{} has schema {}, expected {RUN_SCHEMA}",
                file.display(),
                run["schema"]
            ),
        ));
    }
    Ok(run)
}

/// Every recorded run of one product, newest first.
pub(crate) fn all(harness: &Path, app_id: &str) -> Result<Vec<Json>, Failure> {
    let directory = directory(harness, app_id);
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut runs = Vec::new();
    for entry in entries {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if let Some(run_id) = name.strip_suffix(".json") {
            runs.push(read(harness, app_id, run_id)?);
        }
    }
    runs.sort_by(|left, right| {
        right["startedAt"]
            .as_str()
            .cmp(&left["startedAt"].as_str())
            .then_with(|| right["runId"].as_str().cmp(&left["runId"].as_str()))
    });
    Ok(runs)
}
