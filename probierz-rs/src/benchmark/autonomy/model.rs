//! The two decisions the operator made by hand before, asked of the Stado
//! model router instead: is a scouted product worth building against these
//! rivals on this evidence, and which words name the market one of our
//! products serves, so Trends can watch it. Each answer is judged by its
//! shape and sent back with the reason until it holds or the rounds run out.

use std::path::Path;

use serde_json::{json, Value as Json};

use crate::benchmark::author::drafted;
use crate::failure::{Code, Failure};

const POINT: &str = "benchmark.autonomy";

const RULES: &str = "\
Accept the brief only when all of these hold:
- the trend has at least its evidence floor and its recent window is not below the one before;
- every rival is a product of its own, with its own https site or repository, shown by an observation;
- the gap names something none of the rivals does, and a benchmark case could show it;
- the product is not one our catalog already has, and its description says what it does in one sentence.
Otherwise refuse it, and say which rule it breaks.";

/// Ask one question until `read` accepts the answer, sending each refused
/// answer back with the reason, for at most `rounds` drafts.
fn ask<T>(
    harness: &Path,
    rounds: u32,
    tool: &str,
    question: &str,
    read: impl Fn(&str) -> Result<T, String>,
) -> Result<(T, Vec<Json>), Failure> {
    let mut history = Vec::new();
    let mut previous = String::new();
    for round in 1..=rounds {
        let asked = format!("{question}{previous}");
        let (content, model) = drafted(harness, "autonomy", &asked, tool, "Submit the answer as one JSON document.")?;
        match read(&content) {
            Ok(value) => {
                history.push(json!({"round": round, "model": model}));
                return Ok((value, history));
            }
            Err(why) => {
                history.push(json!({"round": round, "model": model, "rejected": why}));
                previous = format!("\n\nYour previous answer was:\n{content}\nIt was refused: {why}\nAnswer again.");
            }
        }
    }
    Err(Failure::new(
        POINT,
        Code::Refused,
        format!("after {rounds} draft(s) the model gave no {tool} answer that holds:{previous}"),
    ))
}

fn parsed(content: &str) -> Result<Json, String> {
    serde_json::from_str(content).map_err(|error| format!("the answer is not one JSON document: {error}"))
}

/// The model's verdict on one scouted brief: `accept` and its reasons.
pub(super) fn judge(harness: &Path, brief: &Json, rounds: u32) -> Result<Json, Failure> {
    let view = json!({
        "trend": {
            "verdict": brief["trend"]["verdict"],
            "recent": brief["trend"]["recent"],
            "baseline": brief["trend"]["baseline"],
            "evidence": brief["trend"]["evidence"],
            "min_evidence": brief["trend"]["min_evidence"],
        },
        "product": brief["creation"]["product"],
        "gap": brief["gap"],
        "rivals": brief["rivals"],
        "benchmark": brief["benchmark"],
        "evidence": brief["creation"]["evidence_refs"],
    });
    let question = format!(
        "Decide whether we build this product.\n\n{RULES}\n\nThe brief:\n{}\n\nAnswer one JSON document: {{\"accept\": true or false, \"reasons\": [one sentence per rule you checked]}}.",
        serde_json::to_string_pretty(&view).unwrap_or_default()
    );
    let (verdict, rounds_taken) = ask(harness, rounds, "submit_verdict", &question, |content| {
        let answer = parsed(content)?;
        let accept = answer["accept"]
            .as_bool()
            .ok_or_else(|| "the answer has no boolean \"accept\"".to_string())?;
        let reasons: Vec<String> = answer["reasons"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
            .map(str::to_string)
            .collect();
        if reasons.is_empty() {
            return Err("the answer gives no \"reasons\"".to_string());
        }
        Ok(json!({"accept": accept, "reasons": reasons}))
    })?;
    Ok(json!({"accept": verdict["accept"], "reasons": verdict["reasons"], "rounds": rounds_taken}))
}

/// The words Trends matches to watch the market one of our products serves.
pub(super) fn terms(harness: &Path, product: &Json, rounds: u32) -> Result<Vec<String>, Failure> {
    let question = format!(
        "Our product {} ({}): {}\n\nName the short phrases people put in the titles of posts and repositories about the market this product serves: what its users and its rivals talk about, not the product's own name. Answer one JSON document: {{\"terms\": [two to six phrases]}}.",
        product["name"].as_str().unwrap_or_default(),
        product["id"].as_str().unwrap_or_default(),
        product["description"].as_str().unwrap_or_default()
    );
    let (terms, _) = ask(harness, rounds, "submit_terms", &question, |content| {
        let answer = parsed(content)?;
        let terms: Vec<String> = answer["terms"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
            .map(str::trim)
            .filter(|term| !term.is_empty())
            .map(str::to_string)
            .collect();
        if terms.is_empty() {
            return Err("the answer names no \"terms\"".to_string());
        }
        Ok(terms)
    })?;
    Ok(terms)
}
