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
    let output = stado_wait::output(command).map_err(|error| {
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
    let trend = detected["topics"]
        .as_array()
        .and_then(|topics| topics.iter().find(|entry| entry["topic"] == topic))
        .cloned()
        .ok_or_else(|| {
            Failure::config(
                POINT,
                format!("trends detect --topic {topic} answered no verdict for {topic}"),
            )
        })?;
    checked_trend(trend, &format!("trends detect --topic {topic}"))
}

/// The counts a topic is judged buildable on: its evidence and evidence
/// floor, its recent window and the window before.
const TREND_COUNTS: &[&str] = &["evidence", "min_evidence", "recent", "baseline"];

/// A trend whose verdict is text and whose every count is a whole number, or
/// a refusal naming `source` and the field. A count read as anything else
/// was taken as none, and a floor of none let any topic through.
pub(super) fn checked_trend(trend: Json, source: &str) -> Result<Json, Failure> {
    if !trend["verdict"].is_string() {
        return Err(Failure::config(
            POINT,
            format!("{source} gave a trend whose verdict is not text: {}", trend["verdict"]),
        ));
    }
    if let Some(field) = TREND_COUNTS.iter().find(|field| !trend[**field].is_u64()) {
        return Err(Failure::config(
            POINT,
            format!(
                "{source} gave a trend whose {field} is not a whole number: {}",
                trend[*field]
            ),
        ));
    }
    Ok(trend)
}

/// The newest observations Trends matched to the topic.
pub(super) fn observations(topic: &str, limit: usize) -> Result<Vec<Json>, Failure> {
    let listed = answer(
        Command::new(TRENDS_BIN).args([
            "observations",
            "--topic",
            topic,
            "--limit",
            &limit.to_string(),
        ]),
        TRENDS_OWNER,
    )?;
    let seen: Vec<Json> = listed["observations"].as_array().cloned().ok_or_else(|| {
        Failure::new(
            POINT,
            Code::Refused,
            format!("trends observations answered without an observations list: {listed}"),
        )
    })?;
    if seen.is_empty() {
        return Err(Failure::new(
            POINT,
            Code::Refused,
            format!(
                "Trends holds no observation matching topic {topic}; `trends ingest` records them"
            ),
        ));
    }
    Ok(seen)
}

/// The candidates competitors-cli makes of the discovery records, merged
/// and deduplicated by its own identity rules. The records are kept beside
/// the brief, so the input of every candidate can be read again.
pub(super) fn candidates(records: &[Json], file: &Path) -> Result<Vec<Json>, Failure> {
    std::fs::write(
        file,
        serde_json::to_string_pretty(&json!({"records": records}))? + "\n",
    )?;
    let discovered = answer(
        Command::new(COMPETITORS_BIN)
            .arg("discover")
            .arg("--records")
            .arg(file),
        "the competitors CLI of wisent-ai/competitors-cli (its package bin) must be on PATH",
    )?;
    let candidates: Vec<Json> = discovered["candidates"].as_array().cloned().ok_or_else(|| {
        Failure::new(
            POINT,
            Code::Refused,
            format!("competitors discover answered without a candidates list: {discovered}"),
        )
    })?;
    if candidates.is_empty() {
        return Err(Failure::new(
            POINT,
            Code::Refused,
            format!(
                "competitors discover made no candidate of the records in {}",
                file.display()
            ),
        ));
    }
    Ok(candidates)
}
