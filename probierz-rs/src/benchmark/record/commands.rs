//! The benchmark commands: declare-time reads, the run that records, and the
//! projections over recorded runs.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value as Json};

use super::store;
use crate::benchmark::inputs::declare::{declared, Contender};
use crate::benchmark::inputs::suite;
use crate::benchmark::measure::{assess, execute};
use crate::benchmark::RUN_SCHEMA;
use crate::failure::{now_iso, print_json, Answer, Failure};
use crate::manifest;

pub(crate) fn suites(harness: &Path, app_id: &str) -> Answer {
    let manifest = manifest::load(harness, app_id)?;
    let declared = declared(&manifest)?;
    let mut answer = declared.describe();
    let mut judged = Vec::new();
    for (id, file) in &declared.suites {
        judged.push(match suite::load(file, id) {
            Ok(loaded) => json!({
                "id": id,
                "file": file.to_string_lossy(),
                "version": loaded.suite.version,
                "hash": loaded.hash,
                "cases": loaded.suite.cases.len(),
                "repetitions": loaded.suite.repetitions,
            }),
            Err(failure) => json!({
                "id": id,
                "file": file.to_string_lossy(),
                "refused": failure.detail,
            }),
        });
    }
    answer["appId"] = json!(app_id);
    answer["suites"] = Json::Array(judged);
    print_json(&answer)
}

/// The revision a contender's program was built from, when it lives in a
/// git checkout: its commit and whether the tree differs from it.
pub(crate) fn source(contender: &Contender) -> Json {
    let Some(directory) = contender.program.parent() else {
        return Json::Null;
    };
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    match git(&["rev-parse", "HEAD"]) {
        Some(commit) => json!({
            "repository": git(&["rev-parse", "--show-toplevel"]),
            "commit": commit,
            "dirty": git(&["status", "--porcelain"]).map(|status| !status.is_empty()),
        }),
        None => Json::Null,
    }
}

pub(crate) fn run(
    harness: &Path,
    app_id: &str,
    suite_id: &str,
    requested: &[String],
    repetitions: Option<usize>,
) -> Answer {
    let (run, file) = recorded_run(harness, app_id, suite_id, requested, repetitions)?;
    print_json(&json!({
        "runId": run["runId"],
        "file": file.to_string_lossy(),
        "summary": run["summary"],
        "standing": assess::standing(&run),
    }))
}

/// Run the chosen contenders on every case of one suite and record the run;
/// answers the run as written and the file it was written to.
pub(crate) fn recorded_run(
    harness: &Path,
    app_id: &str,
    suite_id: &str,
    requested: &[String],
    repetitions: Option<usize>,
) -> Result<(Json, PathBuf), Failure> {
    let manifest = manifest::load(harness, app_id)?;
    let declared = declared(&manifest)?;
    let file = declared.suite(suite_id)?;
    let loaded = suite::load(file, suite_id)?;
    let values = suite::bound(&loaded)?;
    let contenders = declared.chosen(requested)?;
    for contender in &contenders {
        execute::ready(contender)?;
    }
    let repetitions = repetitions.unwrap_or(loaded.suite.repetitions);
    let started_at = now_iso();
    let mut samples = Vec::new();
    for case in &loaded.suite.cases {
        for repetition in 1..=repetitions {
            for contender in &contenders {
                let attempt = execute::attempt(contender, &loaded, &values, case, repetition)?;
                let sample = assess::sample(contender, case, repetition, attempt);
                eprintln!(
                    "probierz benchmark: {} {} #{repetition}: {} in {} ms",
                    case.id, contender.id, sample["status"], sample["durationMs"]
                );
                samples.push(sample);
            }
        }
    }
    let mut run = json!({
        "schema": RUN_SCHEMA,
        "appId": app_id,
        "probierzVersion": env!("CARGO_PKG_VERSION"),
        "suite": {
            "id": loaded.suite.id,
            "version": loaded.suite.version,
            "hash": loaded.hash,
            "file": file.to_string_lossy(),
            "repetitions": repetitions,
        },
        "ours": declared.ours().id,
        "contenders": contenders.iter().map(|contender| json!({
            "id": contender.id,
            "ours": contender.ours,
            "program": contender.program.to_string_lossy(),
            "args": contender.args,
            "env": contender.env,
            "source": source(contender),
        })).collect::<Vec<_>>(),
        "startedAt": started_at,
        "finishedAt": now_iso(),
        "summary": assess::summary(&contenders, &samples),
        "samples": samples,
    });
    let run_id = store::identity(&started_at, &run);
    run["runId"] = json!(run_id);
    let written = store::write(harness, app_id, &run_id, &run)?;
    Ok((run, written))
}

pub(crate) fn list(
    harness: &Path,
    app_id: &str,
    suite_id: Option<&str>,
    limit: Option<usize>,
) -> Answer {
    manifest::load(harness, app_id)?;
    let runs: Vec<Json> = store::all(harness, app_id)?
        .into_iter()
        .filter(|run| suite_id.is_none_or(|id| run["suite"]["id"] == id))
        .take(limit.unwrap_or(usize::MAX))
        .map(|run| {
            json!({
                "runId": run["runId"],
                "suite": run["suite"],
                "startedAt": run["startedAt"],
                "finishedAt": run["finishedAt"],
                "ours": run["ours"],
                "summary": run["summary"],
            })
        })
        .collect();
    print_json(&json!({"appId": app_id, "runs": runs}))
}

pub(crate) fn show(harness: &Path, app_id: &str, run_id: &str) -> Answer {
    print_json(&store::read(harness, app_id, run_id)?)
}

pub(crate) fn compare(harness: &Path, app_id: &str, baseline: &str, candidate: &str) -> Answer {
    let before = store::read(harness, app_id, baseline)?;
    let after = store::read(harness, app_id, candidate)?;
    if before["suite"]["hash"] != after["suite"]["hash"] {
        return Err(Failure::invalid(
            "benchmark.compare",
            format!(
                "{baseline} ran suite {} version {} ({}) and {candidate} ran suite {} version {} ({}); only runs of the same suite are compared",
                before["suite"]["id"], before["suite"]["version"], before["suite"]["hash"],
                after["suite"]["id"], after["suite"]["version"], after["suite"]["hash"],
            ),
        ));
    }
    let empty = serde_json::Map::new();
    let earlier = before["summary"].as_object().unwrap_or(&empty);
    let later = after["summary"].as_object().unwrap_or(&empty);
    let ratio = |field: &str, left: &Json, right: &Json| match (
        left[field].as_f64(),
        right[field].as_f64(),
    ) {
        (Some(base), Some(next)) if base > 0.0 => json!(next / base),
        _ => Json::Null,
    };
    let mut contenders = Vec::new();
    for (id, measured) in later {
        if let Some(base) = earlier.get(id) {
            contenders.push(json!({
                "contender": id,
                "passRatePoints": (measured["passRate"].as_f64().unwrap_or(0.0)
                    - base["passRate"].as_f64().unwrap_or(0.0)) * 100.0,
                "p50Ratio": ratio("p50Ms", base, measured),
                "p95Ratio": ratio("p95Ms", base, measured),
                "baseline": base,
                "candidate": measured,
            }));
        }
    }
    let only = |left: &serde_json::Map<String, Json>,
                right: &serde_json::Map<String, Json>|
     -> Vec<String> {
        left.keys()
            .filter(|id| !right.contains_key(*id))
            .cloned()
            .collect()
    };
    print_json(&json!({
        "suite": after["suite"],
        "baseline": baseline,
        "candidate": candidate,
        "contenders": contenders,
        "onlyInBaseline": only(earlier, later),
        "onlyInCandidate": only(later, earlier),
    }))
}

/// The standing of the newest recorded run of one suite.
pub(crate) fn standing_of(harness: &Path, app_id: &str, suite_id: &str) -> Result<Json, Failure> {
    let manifest = manifest::load(harness, app_id)?;
    declared(&manifest)?.suite(suite_id)?;
    let newest = store::all(harness, app_id)?
        .into_iter()
        .find(|run| run["suite"]["id"] == suite_id)
        .ok_or_else(|| {
            Failure::invalid(
                "benchmark.standing",
                format!(
                    "no run of suite {suite_id} is recorded for {app_id}; probierz benchmark run {app_id} --suite {suite_id} records one"
                ),
            )
        })?;
    Ok(assess::standing(&newest))
}

pub(crate) fn standing(harness: &Path, app_id: &str, suite_id: &str) -> Answer {
    print_json(&standing_of(harness, app_id, suite_id)?)
}
