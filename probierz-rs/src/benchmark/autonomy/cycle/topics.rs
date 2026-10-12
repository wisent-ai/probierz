//! Which topics a cycle scouts: the ones Trends lists, after each catalog
//! product it does not watch yet is given a topic with one source of every
//! query kind; or, on a machine without Trends and Competitors, the
//! `<topic>.json` files of the policy's evidence directory.

use std::path::Path;

use serde_json::{json, Value as Json};

use super::super::{model, policy, steps, POINT, TRENDS};
use crate::benchmark::scout::identity;
use crate::failure::Failure;

/// The source kinds Trends asks by query, read from the sources it already
/// has: a new topic gets one source of each, asked with its first term.
fn query_kinds(harness: &Path) -> Vec<String> {
    let listed = steps::run(Path::new(TRENDS), &steps::command("source list"), harness);
    let mut kinds: Vec<String> = listed["answer"]["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|source| source["query"].is_string())
        .filter_map(|source| source["kind"].as_str())
        .map(str::to_string)
        .collect();
    kinds.sort();
    kinds.dedup();
    kinds
}

/// A Trends topic, with one source of every query kind Trends uses, for each
/// catalog product Trends does not watch yet; the terms are the model's.
pub(in crate::benchmark::autonomy) fn watch(
    harness: &Path,
    products: &[Json],
    policy: &policy::Policy,
    present: &[String],
) -> Vec<Json> {
    let trends = Path::new(TRENDS);
    let kinds = query_kinds(harness);
    let mut added = Vec::new();
    for product in products {
        let id = product["id"].as_str().unwrap_or_default();
        if !identity(id) || present.iter().any(|name| name == id) {
            continue;
        }
        let terms = match model::terms(harness, product, policy.rounds) {
            Ok(terms) => terms,
            Err(failure) => {
                added.push(json!({"product": id, "refusal": failure.detail}));
                continue;
            }
        };
        let mut topic = steps::command("topic add");
        topic.push(id.to_string());
        for term in &terms {
            topic.push("--term".to_string());
            topic.push(term.clone());
        }
        let made = steps::run(trends, &topic, harness);
        let mut sources = Vec::new();
        for kind in &kinds {
            let source = format!("{id}-{kind}");
            let mut add = steps::command("source add");
            add.extend(steps::args(&[
                source.as_str(),
                "--kind",
                kind.as_str(),
                "--query",
                terms[0].as_str(),
            ]));
            sources.push(steps::run(trends, &add, harness));
        }
        added.push(json!({"product": id, "terms": terms, "topic": made, "sources": sources}));
    }
    added
}

/// The topic names a Trends `topic list` answer carries.
pub(in crate::benchmark::autonomy) fn topic_names(answer: &Json) -> Vec<String> {
    answer["topics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|topic| topic["name"].as_str())
        .map(str::to_string)
        .collect()
}

/// The topics an evidence directory holds: one `<topic>.json` each, in name
/// order; a directory that cannot be listed, or holds no such file, is refused.
pub(in crate::benchmark::autonomy) fn evidence_topics(
    directory: &Path,
) -> Result<Vec<String>, Failure> {
    let entries = std::fs::read_dir(directory).map_err(|error| {
        Failure::config(
            POINT,
            format!(
                "the evidence directory {} cannot be listed: {error}",
                directory.display()
            ),
        )
    })?;
    let mut topics = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            Failure::config(
                POINT,
                format!(
                    "an entry of the evidence directory {} cannot be read: {error}",
                    directory.display()
                ),
            )
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(topic) = name.strip_suffix(".json") {
            topics.push(topic.to_string());
        }
    }
    if topics.is_empty() {
        return Err(Failure::config(
            POINT,
            format!(
                "the evidence directory {} holds no <topic>.json file",
                directory.display()
            ),
        ));
    }
    topics.sort();
    Ok(topics)
}
