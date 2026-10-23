//! The report: what was judged, by digest, and the verdict — written once.

use super::*;

/// The kind of document this writes.
const REPORT_KIND: &str = "probierz-talk-evaluation";

/// Every input of the evaluation by path and the SHA-256 of the bytes read
/// once into the talk, the bytes the model was given; and the rubric.
pub(crate) fn identity_document(talk: &Talk, rubric: &JsonValue) -> JsonValue {
    let digest = |path: &Path, bytes: &[u8]| {
        json!({
            "path": path.display().to_string(),
            "sha256": crate::evidence::sha256_bytes(bytes),
        })
    };
    let slides: Vec<JsonValue> = talk
        .slides
        .iter()
        .map(|slide| digest(&slide.path, &slide.bytes))
        .collect();
    let sources: Vec<JsonValue> = talk
        .sources
        .iter()
        .map(|source| digest(&source.path, &source.bytes))
        .collect();
    json!({
        "slides": slides,
        "script": digest(&talk.script, &talk.script_bytes),
        "sources": sources,
        "rubric": rubric
    })
}

/// The verdict: a pass only when every criterion is met and the model named
/// no blocker; every unmet criterion and every blocker is kept.
pub(crate) fn talk_report(identity: JsonValue, graded: Graded) -> JsonValue {
    let named: Vec<JsonValue> = graded.evaluation["blockers"]
        .as_array()
        .into_iter()
        .flatten()
        .cloned()
        .collect();
    let blockers: Vec<JsonValue> = graded.unmet.into_iter().chain(named).collect();
    json!({
        "kind": REPORT_KIND,
        "evaluatedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "inputs": identity,
        "model": graded.model,
        "evaluation": graded.evaluation,
        "verdict": { "pass": blockers.is_empty(), "blockers": blockers }
    })
}

/// Write the report exactly once: a path that already exists is refused,
/// never overwritten.
pub(crate) fn write_talk_report(report: &Path, value: &JsonValue) -> Result<(), Failure> {
    let mut file = File::create_new(report).map_err(|error| {
        Failure::invalid(
            "evaluate.talk",
            format!("the talk evaluation report {}: {error}", report.display()),
        )
    })?;
    let text = serde_json::to_string_pretty(value).map_err(|error| {
        Failure::config(
            "evaluate.talk",
            format!("the talk evaluation cannot be written as JSON: {error}"),
        )
    })?;
    file.write_all(text.as_bytes()).map_err(|error| {
        Failure::invalid(
            "evaluate.talk",
            format!("the talk evaluation report {}: {error}", report.display()),
        )
    })?;
    Ok(())
}
