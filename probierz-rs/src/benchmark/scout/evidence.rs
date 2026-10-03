//! What a scout reads from the products that own the evidence: the trend
//! Trends measured for a topic, the observations behind it, and the
//! candidates competitors-cli normalises from the products a model found in
//! those observations. Probierz interprets none of it a second time.

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value as Json};

use crate::failure::{ended, Code, Failure};

const POINT: &str = "benchmark.scout";
const TRENDS_BIN: &str = "trends";
const COMPETITORS_BIN: &str = "competitors";

/// Run one command, answer its JSON stdout, or say who refused and why.
fn answer(command: &mut Command, owner: &str) -> Result<Json, Failure> {
    let described = format!("{command:?}");
    let output = command.output().map_err(|error| {
        Failure::new(
            POINT,
            Code::Prerequisite,
            format!("{described} could not start ({error}); {owner}"),
        )
    })?;
    if !output.status.success() {
        return Err(Failure::invalid(
            POINT,
            format!(
                "{described} {}: {}",
                ended(&output.status),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        Failure::config(POINT, format!("{described} did not answer JSON: {error}"))
    })
}

const TRENDS_OWNER: &str = "the trends CLI of wisent-ai/trends must be on PATH, with TRENDS_STATE_FILE naming its state when it is not the default";

/// The verdict Trends reads for one topic, with the arithmetic behind it.
pub(super) fn trend(topic: &str) -> Result<Json, Failure> {
    let detected = answer(
        Command::new(TRENDS_BIN).args(["detect", "--topic", topic]),
        TRENDS_OWNER,
    )?;
    detected["topics"]
        .as_array()
        .and_then(|topics| topics.iter().find(|entry| entry["topic"] == topic))
        .cloned()
        .ok_or_else(|| {
            Failure::config(POINT, format!("trends detect --topic {topic} answered no verdict for {topic}"))
        })
}

/// The newest observations Trends matched to the topic.
pub(super) fn observations(topic: &str, limit: usize) -> Result<Vec<Json>, Failure> {
    let listed = answer(
        Command::new(TRENDS_BIN).args(["observations", "--topic", topic, "--limit", &limit.to_string()]),
        TRENDS_OWNER,
    )?;
    let seen: Vec<Json> = listed["observations"].as_array().cloned().unwrap_or_default();
    if seen.is_empty() {
        return Err(Failure::new(
            POINT,
            Code::Refused,
            format!("Trends holds no observation matching topic {topic}; `trends ingest` records them"),
        ));
    }
    Ok(seen)
}

/// The candidates competitors-cli makes of the discovery records, merged
/// and deduplicated by its own identity rules. The records are kept beside
/// the brief, so the input of every candidate can be read again.
pub(super) fn candidates(records: &[Json], file: &Path) -> Result<Vec<Json>, Failure> {
    std::fs::write(file, serde_json::to_string_pretty(&json!({"records": records}))? + "\n")?;
    let discovered = answer(
        Command::new(COMPETITORS_BIN)
            .arg("discover")
            .arg("--records")
            .arg(file),
        "the competitors CLI of wisent-ai/competitors-cli (its package bin) must be on PATH",
    )?;
    let candidates: Vec<Json> = discovered["candidates"].as_array().cloned().unwrap_or_default();
    if candidates.is_empty() {
        return Err(Failure::new(
            POINT,
            Code::Refused,
            format!("competitors discover made no candidate of the records in {}", file.display()),
        ));
    }
    Ok(candidates)
}
