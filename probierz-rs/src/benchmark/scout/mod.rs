//! Scouting a new product, so that it starts life measured against the
//! products it has to beat.
//!
//! `scout` reads what Trends measured for one watched topic and the dated
//! observations behind it, asks the Stado model router which products those
//! observations show serving the topic, normalises them through
//! competitors-cli, and asks for an opportunity: the product we would build,
//! the rivals it has to beat among those candidates, and the suite that
//! measures it. Every rival and every claim cites an observation; a draft
//! that cites anything else is sent back. The brief is written once under
//! `test-results/.scout/<topic>/` and holds the Stado creation request.
//!
//! `adopt` is the operator's decision on one brief: Stado creates the private
//! repository and the preview catalog record, the catalog names the rivals
//! and the benchmark, Probierz declares the product's manifest and drafts its
//! first suite. From there the benchmark loop runs as for every product.

mod adopt;
mod brief;
mod evidence;

use std::path::Path;

use serde_json::{json, Value as Json};

use super::author::drafted;
use super::record::catalog;
use crate::failure::{print_json, Answer, Code, Failure};

pub(crate) use adopt::adopt;

pub(crate) const BRIEF_SCHEMA: &str = "ai.wisent.probierz.benchmark.opportunity.v1";
const POINT: &str = "benchmark.scout";

/// A Stado identity: lowercase letters, digits and inner hyphens, starting
/// with a letter, at most 100 bytes.
pub(crate) fn identity(value: &str) -> bool {
    value.len() <= 100
        && value.starts_with(|c: char| c.is_ascii_lowercase())
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// Draft one structured answer until `read` accepts it, sending each
/// rejected draft back with what was wrong, for at most `rounds` drafts.
fn until_read<T>(
    harness: &Path,
    rounds: u32,
    tool: &str,
    description: &str,
    brief: impl Fn(Option<(&str, &str)>) -> String,
    read: impl Fn(&str) -> Result<T, String>,
) -> Result<(T, Vec<Json>), Failure> {
    let mut history = Vec::new();
    let mut rejected: Option<(String, String)> = None;
    for round in 1..=rounds {
        let asked = brief(
            rejected
                .as_ref()
                .map(|(draft, why)| (draft.as_str(), why.as_str())),
        );
        let (content, model) = drafted(harness, "scout", &asked, tool, description)?;
        match read(&content) {
            Ok(value) => {
                history.push(json!({"round": round, "model": model}));
                return Ok((value, history));
            }
            Err(why) => {
                history.push(json!({"round": round, "model": model, "rejected": why}));
                rejected = Some((content, why));
            }
        }
    }
    Err(Failure::new(
        POINT,
        Code::Refused,
        format!(
            "after {rounds} draft(s) no {tool} answer cited only what it was given: {}",
            rejected.map(|(_, why)| why).unwrap_or_default()
        ),
    ))
}

/// A topic a product can be built on: Trends holds at least its evidence
/// floor for it, and the recent window is not below the one before.
fn buildable(trend: &Json) -> bool {
    let count = |key: &str| trend[key].as_u64().unwrap_or_default();
    count("evidence") >= count("min_evidence") && count("recent") >= count("baseline")
}

pub(crate) fn scout(
    harness: &Path,
    topic: &str,
    owner: &str,
    observations: usize,
    rounds: u32,
) -> Answer {
    if !identity(topic) || !identity(owner) {
        return Err(Failure::invalid(
            POINT,
            format!("topic {topic:?} and --owner {owner:?} must be lowercase letters, digits and inner hyphens, starting with a letter: both become Stado identities"),
        ));
    }
    if observations == 0 || rounds == 0 {
        return Err(Failure::invalid(
            POINT,
            "--observations and --rounds must be at least 1",
        ));
    }
    let trend = evidence::trend(topic)?;
    if !buildable(&trend) {
        print_json(&json!({"topic": topic, "trend": trend}))?;
        return Err(Failure::new(
            POINT,
            Code::Refused,
            format!(
                "Trends reads topic {topic} as {}: {} observations against a floor of {}, {} recent against {} before; a product is scouted on a topic with enough evidence that is not falling. `trends ingest` adds evidence",
                trend["verdict"].as_str().unwrap_or("unknown"), trend["evidence"], trend["min_evidence"], trend["recent"], trend["baseline"]
            ),
        ));
    }
    let seen = evidence::observations(topic, observations)?;
    let (records, extracted) = until_read(
        harness,
        rounds,
        "submit_products_seen",
        "Submit the products the observations show, as JSON.",
        |rejected| brief::products(topic, &seen, rejected),
        |content| brief::records(content, topic, &seen),
    )?;
    let directory = harness.join("test-results").join(".scout").join(topic);
    std::fs::create_dir_all(&directory)?;
    let stamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let brief_id = format!("scout-{topic}-{stamp}");
    let candidates = evidence::candidates(
        &records,
        &directory.join(format!("{brief_id}.records.json")),
    )?;
    let ours = catalog::products()?;
    let (opportunity, decided) = until_read(
        harness,
        rounds,
        "submit_opportunity",
        "Submit the opportunity as JSON.",
        |rejected| brief::opportunity(topic, &trend, &candidates, &ours, rejected),
        |content| brief::opportunity_of(content, &candidates, &seen, &ours),
    )?;
    let document = brief::document(
        &brief::Scouted {
            id: &brief_id,
            topic,
            owner,
            trend: &trend,
            seen: &seen,
            candidates: &candidates,
        },
        &opportunity,
        json!({"products": extracted, "opportunity": decided}),
    );
    let file = directory.join(format!("{brief_id}.json"));
    std::fs::write(&file, serde_json::to_string_pretty(&document)? + "\n")?;
    print_json(&json!({
        "brief": file.to_string_lossy(),
        "trend": {"verdict": trend["verdict"], "recent": trend["recent"], "baseline": trend["baseline"]},
        "product": document["creation"]["product"],
        "rivals": document["rivals"],
        "benchmark": document["benchmark"],
        "next": format!("probierz benchmark adopt {} --allow-create", file.display()),
    }))
}
