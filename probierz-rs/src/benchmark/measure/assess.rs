//! Judging answers and measuring contenders. No composite score: a contender
//! is described by how often it passed and how long it took, and a case is
//! won by passing more often, then by the shorter median.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use serde_json::{json, Value as Json};

use super::execute::Attempt;
use crate::benchmark::inputs::declare::Contender;
use crate::benchmark::inputs::suite::{Assertion, Case};

/// The assertions an output fails, each with what was found.
fn failed(assertions: &[Assertion], output: &Json) -> Vec<Json> {
    let mut failures = Vec::new();
    for assertion in assertions {
        let found = output.pointer(&assertion.pointer);
        let holds = if let Some(expected) = &assertion.equals {
            found == Some(expected)
        } else if let Some(exists) = assertion.exists {
            found.is_some() == exists
        } else if let Some(needle) = &assertion.includes {
            match found {
                Some(Json::String(text)) => text.contains(needle.as_str()),
                Some(Json::Array(items)) => items
                    .iter()
                    .any(|item| item.as_str() == Some(needle.as_str())),
                _ => false,
            }
        } else {
            unreachable!("suite::load refuses an assertion without a check")
        };
        if !holds {
            failures.push(json!({
                "assertion": assertion,
                "found": found.cloned().unwrap_or(Json::Null),
                "present": found.is_some(),
            }));
        }
    }
    failures
}

pub(crate) fn sample(
    contender: &Contender,
    case: &Case,
    repetition: usize,
    attempt: Attempt,
) -> Json {
    let failures = if attempt.completed {
        failed(&case.assertions, &attempt.output)
    } else {
        Vec::new()
    };
    let status = match (&attempt.broken, attempt.completed) {
        (Some(_), _) => "broken",
        (None, false) => "failed",
        (None, true) => "completed",
    };
    let passed = attempt.completed && failures.is_empty();
    json!({
        "contender": contender.id,
        "case": case.id,
        "repetition": repetition,
        "status": status,
        "passed": passed,
        "durationMs": attempt.duration_ms,
        "ended": attempt.ended,
        "broken": attempt.broken,
        "error": attempt.error,
        "failedAssertions": failures,
        "steps": attempt.steps,
        "tokens": attempt.tokens,
        "costUsd": attempt.cost_usd,
        "output": attempt.output,
    })
}

/// Nearest-rank percentile of sorted durations.
fn percentile(sorted: &[u64], percent: u64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (percent as usize * sorted.len()).div_ceil(100).max(1);
    Some(sorted[rank - 1])
}

fn measured<'a>(samples: impl Iterator<Item = &'a Json>) -> Json {
    let samples: Vec<&Json> = samples.collect();
    let mut durations: Vec<u64> = samples
        .iter()
        .filter_map(|sample| sample["durationMs"].as_u64())
        .collect();
    durations.sort_unstable();
    let passed = samples
        .iter()
        .filter(|sample| sample["passed"] == true)
        .count();
    let total = |field: &str| -> Json {
        let reported: Vec<f64> = samples
            .iter()
            .filter_map(|sample| sample[field].as_f64())
            .collect();
        if reported.is_empty() {
            Json::Null
        } else {
            json!({"sum": reported.iter().sum::<f64>(), "reportedBy": reported.len()})
        }
    };
    json!({
        "attempts": samples.len(),
        "passed": passed,
        "passRate": if samples.is_empty() { 0.0 } else { passed as f64 / samples.len() as f64 },
        "broken": samples.iter().filter(|sample| sample["status"] == "broken").count(),
        "p50Ms": percentile(&durations, 50),
        "p95Ms": percentile(&durations, 95),
        "p99Ms": percentile(&durations, 99),
        "steps": total("steps"),
        "tokens": total("tokens"),
        "costUsd": total("costUsd"),
    })
}

/// Per contender over the whole run.
pub(crate) fn summary(contenders: &[Contender], samples: &[Json]) -> Json {
    let mut by_contender = serde_json::Map::new();
    for contender in contenders {
        let mine = samples
            .iter()
            .filter(|sample| sample["contender"] == contender.id.as_str());
        by_contender.insert(contender.id.clone(), measured(mine));
    }
    Json::Object(by_contender)
}

/// Who won each case of one recorded run, and every case ours did not win.
pub(crate) fn standing(run: &Json) -> Json {
    let ours = run["ours"].as_str().unwrap_or_default().to_string();
    let samples = run["samples"].as_array().cloned().unwrap_or_default();
    let mut cases: BTreeMap<String, BTreeMap<String, Vec<&Json>>> = BTreeMap::new();
    for sample in &samples {
        let case = sample["case"].as_str().unwrap_or_default().to_string();
        let contender = sample["contender"].as_str().unwrap_or_default().to_string();
        cases
            .entry(case)
            .or_default()
            .entry(contender)
            .or_default()
            .push(sample);
    }
    let mut verdicts = Vec::new();
    let mut losses = Vec::new();
    for (case, contenders) in &cases {
        let by_contender: BTreeMap<&String, Json> = contenders
            .iter()
            .map(|(id, samples)| (id, measured(samples.iter().copied())))
            .collect();
        let key = |value: &Json| {
            (
                value["passRate"].as_f64().unwrap_or(0.0),
                Reverse(value["p50Ms"].as_u64().unwrap_or(u64::MAX)),
            )
        };
        let best =
            by_contender
                .values()
                .map(key)
                .fold(None, |best: Option<(f64, Reverse<u64>)>, next| match best {
                    Some(current) if current >= next => Some(current),
                    _ => Some(next),
                });
        let winners: Vec<&String> = by_contender
            .iter()
            .filter(|(_, value)| Some(key(value)) == best)
            .map(|(id, _)| *id)
            .collect();
        let verdict = match (winners.iter().any(|id| **id == ours), winners.len()) {
            (true, 1) => "won",
            (true, _) => "tied",
            (false, _) if !by_contender.contains_key(&ours) => "absent",
            (false, _) => "lost",
        };
        let entry = json!({
            "case": case,
            "verdict": verdict,
            "winners": winners,
            "contenders": by_contender,
        });
        if verdict == "lost" {
            losses.push(json!({
                "case": case,
                "winners": winners,
                "ours": by_contender.get(&ours),
                "winner": by_contender.get(winners[0]),
            }));
        }
        verdicts.push(entry);
    }
    json!({
        "runId": run["runId"],
        "suite": run["suite"],
        "ours": ours,
        "cases": verdicts,
        "losses": losses,
    })
}
