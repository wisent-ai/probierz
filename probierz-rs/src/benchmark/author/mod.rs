//! Authoring the benchmark itself, for any product: the suites it runs and the
//! contender drivers that answer them, drafted through the Stado model
//! router (Brama) instead of written by hand for one product.
//!
//! `author` drafts the driver of one contender, our product or a rival the
//! catalog names, from its catalog record and the suite. The driver is placed
//! in the product's tree, declared in its manifest, and verified by a
//! recorded run of that contender alone. An attempt that breaks the contract
//! or fails with an error goes back to the model with the driver and what the
//! run found, for a bounded number of rounds. A wrong answer is not repaired:
//! it is the measurement. A driver already declared is verified first and
//! redrafted only when it fails, so `author` is also how a driver a rival's
//! new release broke is repaired.
//!
//! `author-suite` drafts a suite from the catalog record of the product and
//! its rivals, judges it with the suite loader, writes it once and declares
//! it.

mod brief;
mod place;

use std::path::Path;

use serde_json::{json, Value as Json};

use crate::authoring::draft_structured_artifact;
use crate::benchmark::inputs::suite;
use crate::benchmark::record::{catalog, commands};
use crate::failure::{print_json, Answer, Code, Failure};
use crate::manifest;
use brief::{Previous, Subject};

fn drafted(
    harness: &Path,
    app_id: &str,
    brief: &str,
    tool: &str,
    description: &str,
) -> Result<(String, Json), Failure> {
    let reply = draft_structured_artifact(harness, app_id, None, brief, tool, description)
        .map_err(|error| Failure::unavailable("benchmark.author", error))?;
    Ok((
        reply.content,
        json!({"model": reply.model, "usage": reply.usage}),
    ))
}

/// What the catalog records about the contender being authored.
fn subject(entry: &Json, product: &str, id: &str, ours: bool) -> Result<Subject, Failure> {
    if ours {
        let record = json!({
            "id": entry["id"],
            "name": entry["name"],
            "description": entry["description"],
            "docs_origin": entry["docs_origin"],
            "surfaces": entry["surfaces"],
        });
        return Ok(Subject { ours, record });
    }
    let rival = entry["rivals"]
        .as_array()
        .and_then(|rivals| rivals.iter().find(|rival| rival["id"] == id))
        .cloned()
        .ok_or_else(|| {
            Failure::invalid(
                "benchmark.author",
                format!("the catalog names no rival {id} for {product}; stado product registry set {product} --add-rival declares one, or --ours authors our own contender"),
            )
        })?;
    Ok(Subject {
        ours,
        record: rival,
    })
}

/// What one recorded run of a single contender found wrong with its driver:
/// broken attempts and failed ones, never wrong answers.
fn verify(
    harness: &Path,
    app_id: &str,
    suite_id: &str,
    id: &str,
) -> Result<(Json, Vec<String>), Failure> {
    let (run, file) =
        commands::recorded_run(harness, app_id, suite_id, &[id.to_string()], Some(1))?;
    let failures = run["samples"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sample| match sample["status"].as_str() {
            Some("broken") => Some(format!(
                "case {}: broke the contract: {}",
                sample["case"], sample["broken"]
            )),
            Some("failed") => Some(format!(
                "case {}: answered failed: {}",
                sample["case"], sample["error"]
            )),
            _ => None,
        })
        .collect();
    Ok((
        json!({"runId": run["runId"], "file": file.to_string_lossy(), "summary": run["summary"]}),
        failures,
    ))
}

pub(crate) fn contender(
    harness: &Path,
    app_id: &str,
    id: &str,
    suite_id: &str,
    ours: bool,
    rounds: u32,
) -> Answer {
    if rounds == 0 {
        return Err(Failure::invalid(
            "benchmark.author",
            "--rounds must be at least 1",
        ));
    }
    let manifest = manifest::load(harness, app_id)?;
    let product = catalog::product_id(&manifest);
    let entry = catalog::record(&product)?;
    let subject = subject(&entry, &product, id, ours)?;
    let loaded = suite::load(&place::suite_file(&manifest, suite_id)?, suite_id)?;
    match place::ours(&manifest) {
        Some(declared) if ours && declared != id => {
            return Err(Failure::invalid(
                "benchmark.author",
                format!("{declared} is already our contender; one product has one"),
            ))
        }
        None if !ours => {
            return Err(Failure::new(
                "benchmark.author",
                Code::Prerequisite,
                format!("no contender is ours yet, so no rival can be measured; probierz benchmark author {app_id} --contender <id> --ours --suite {suite_id} drafts ours first"),
            ))
        }
        _ => {}
    }
    let mut history = Vec::new();
    let mut previous = None;
    if let Some(program) = place::program(&manifest, id) {
        if let Ok(source) = std::fs::read_to_string(&program) {
            let (run, failures) = verify(harness, app_id, suite_id, id)?;
            history.push(json!({"round": 0, "run": run, "failures": failures}));
            if failures.is_empty() {
                return print_json(
                    &json!({"appId": app_id, "contender": id, "program": program.to_string_lossy(), "accepted": true, "rounds": history}),
                );
            }
            previous = Some(Previous { source, failures });
        }
    }
    for round in 1..=rounds {
        let brief = brief::contender(&subject, &loaded, previous.as_ref(), round, rounds);
        let (content, model) = drafted(
            harness,
            app_id,
            &brief,
            "submit_probierz_contender",
            "Submit the complete benchmark contender driver as a {file, env, source} JSON object.",
        )?;
        let driver = match brief::driver(&content, ours) {
            Ok(driver) => driver,
            Err(reason) => {
                history.push(json!({"round": round, "model": model, "failures": [reason.clone()]}));
                previous = Some(Previous {
                    source: content,
                    failures: vec![reason],
                });
                continue;
            }
        };
        let program = place::driver(&manifest, id, ours, &driver)?;
        place::declare(&manifest, id, ours, &program, &driver.env)?;
        let (run, failures) = verify(harness, app_id, suite_id, id)?;
        history.push(json!({"round": round, "model": model, "program": program.to_string_lossy(), "env": driver.env, "run": run, "failures": failures}));
        if failures.is_empty() {
            return print_json(
                &json!({"appId": app_id, "contender": id, "program": program.to_string_lossy(), "accepted": true, "rounds": history}),
            );
        }
        previous = Some(Previous {
            source: driver.source,
            failures,
        });
    }
    print_json(&json!({"appId": app_id, "contender": id, "accepted": false, "rounds": history}))?;
    let last = previous
        .map(|previous| previous.failures.join("; "))
        .unwrap_or_default();
    Err(Failure::new(
        "benchmark.author",
        Code::Refused,
        format!(
            "after {rounds} round(s) the {id} driver still breaks the contract or fails: {last}"
        ),
    ))
}

pub(crate) fn suite(
    harness: &Path,
    app_id: &str,
    suite_id: &str,
    cases: usize,
    rounds: u32,
) -> Answer {
    if rounds == 0 || cases == 0 {
        return Err(Failure::invalid(
            "benchmark.author",
            "--rounds and --cases must be at least 1",
        ));
    }
    let manifest = manifest::load(harness, app_id)?;
    let product = catalog::product_id(&manifest);
    let entry = catalog::record(&product)?;
    let target = place::suite_target(&manifest, suite_id)?;
    std::fs::create_dir_all(target.parent().expect("a suite file sits in benchmark/"))?;
    let mut history = Vec::new();
    let mut previous = None;
    for round in 1..=rounds {
        let brief = brief::suite(&entry, suite_id, cases, previous.as_ref());
        let (content, model) = drafted(
            harness,
            app_id,
            &brief,
            "submit_probierz_suite",
            "Submit the complete benchmark suite JSON document.",
        )?;
        std::fs::write(&target, &content)?;
        match suite::load(&target, suite_id) {
            Ok(loaded) => {
                place::declare_suite(&manifest, suite_id, &target)?;
                history.push(json!({"round": round, "model": model}));
                return print_json(&json!({
                    "appId": app_id,
                    "suite": suite_id,
                    "file": target.to_string_lossy(),
                    "hash": loaded.hash,
                    "cases": loaded.suite.cases.len(),
                    "variables": loaded.suite.variables,
                    "rounds": history,
                }));
            }
            Err(refusal) => {
                std::fs::remove_file(&target)?;
                let reason = refusal.to_string();
                history.push(json!({"round": round, "model": model, "failures": [reason.clone()]}));
                previous = Some(Previous {
                    source: content,
                    failures: vec![reason],
                });
            }
        }
    }
    print_json(&json!({"appId": app_id, "suite": suite_id, "accepted": false, "rounds": history}))?;
    Err(Failure::new(
        "benchmark.author",
        Code::Refused,
        format!("after {rounds} round(s) no draft of suite {suite_id} passed the suite loader"),
    ))
}
