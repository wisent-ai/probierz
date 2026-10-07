//! The loop from a lost case to a won one.
//!
//! A case the newest run lost becomes one durable Jeden pursuit request in
//! our product's own checkout: the case, who beat us, by how much, and the
//! finish line. Jeden and Pursuit establish the contract, execute it and
//! review it. Their verdict does not close the case. Probierz then records a
//! new run of the whole suite itself, and only that run's standing decides:
//! the case is accepted when ours wins or ties it, and the catalog roadmap is
//! brought in line with that run, which withdraws the item of a case ours now
//! wins. A pursuit that reports success while the new run still loses the case
//! is refused, so no agent's own claim closes a loss.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value as Json};

use super::{catalog, commands};
use crate::benchmark::inputs::declare::declared;
use crate::benchmark::inputs::suite;
use crate::benchmark::measure::assess;
use crate::failure::{create_private, print_json, Answer, Code, Failure};
use crate::manifest;

const POINT: &str = "benchmark.pursue";
const JEDEN_BIN: &str = "jeden";
const REQUEST_SCHEMA_VERSION: u32 = 1;

/// Hand one lost case of the newest run to Jeden, then decide it with a run
/// Probierz records itself.
pub(crate) fn pursue(
    harness: &Path,
    app_id: &str,
    suite_id: &str,
    case_id: &str,
    budget_usd: &str,
) -> Answer {
    if !budget_usd
        .parse::<f64>()
        .is_ok_and(|budget| budget.is_finite() && budget > 0.0)
    {
        return Err(Failure::invalid(
            POINT,
            format!("--budget-usd {budget_usd} is not a positive amount of US dollars"),
        ));
    }
    let manifest = manifest::load(harness, app_id)?;
    let declared = declared(&manifest)?;
    let ours = declared.ours().clone();
    let file = declared.suite(suite_id)?.clone();
    let loaded = suite::load(&file, suite_id)?;
    let case = loaded
        .suite
        .cases
        .iter()
        .find(|case| case.id == case_id)
        .ok_or_else(|| {
            let known: Vec<&str> = loaded
                .suite
                .cases
                .iter()
                .map(|case| case.id.as_str())
                .collect();
            Failure::invalid(
                POINT,
                format!(
                    "suite {suite_id} holds no case {case_id}; its cases are {}",
                    known.join(", ")
                ),
            )
        })?;
    let standing = commands::standing_of(harness, app_id, suite_id)?;
    let run_id = standing["runId"].as_str().unwrap_or_default().to_string();
    let loss = standing["losses"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|loss| loss["case"] == case_id)
        .cloned()
        .ok_or_else(|| {
            Failure::invalid(
                POINT,
                format!("run {run_id} does not lose case {case_id} (its verdict is {}); nothing to pursue", verdict(&standing, case_id)),
            )
        })?;
    let checkout = commands::source(&ours)["repository"]
        .as_str()
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok())
        .ok_or_else(|| {
            Failure::config(
                POINT,
                format!("our contender {} at {} is not in a git checkout; a pursuit changes our product in its checkout", ours.id, ours.program.display()),
            )
        })?;
    let objective = objective(app_id, suite_id, &run_id, &ours.id, &file, case, &loss)?;
    let request_id = identifier(&format!("{run_id}-{case_id}"));
    let directory = harness
        .join("test-results")
        .join(".benchmark")
        .join(app_id)
        .join("pursuits");
    std::fs::create_dir_all(&directory)?;
    let request_file = directory.join(format!("{request_id}.json"));
    if !request_file.exists() {
        let request = json!({
            "schema_version": REQUEST_SCHEMA_VERSION,
            "request_id": request_id,
            "initiative_id": identifier(&format!("benchmark-{app_id}-{suite_id}-{case_id}")),
            "objective": objective,
            "cwd": checkout,
            "evidence_refs": [harness.join("test-results").join(".benchmark").join(app_id).join(format!("{run_id}.json"))],
            "budget_usd": budget_usd,
            "allow_write": true,
            "allow_command": true,
        });
        std::io::Write::write_all(
            &mut create_private(&request_file)?,
            serde_json::to_string_pretty(&request)?.as_bytes(),
        )?;
    }
    let pursuit = jeden(&request_file)?;
    let state = pursuit["state"].as_str().unwrap_or("unknown").to_string();
    if state != "succeeded" {
        print_json(
            &json!({"appId": app_id, "case": case_id, "request": request_file, "pursuit": pursuit}),
        )?;
        return Err(Failure::new(
            POINT,
            Code::Refused,
            format!(
                "Jeden's pursuit {request_id} ended {state}: {}; case {case_id} stays lost, and `jeden pursue --resume-run {request_id} --json` resumes a blocked one",
                pursuit["error"].as_str().unwrap_or("no error was reported")
            ),
        ));
    }
    let (run, run_file) = commands::recorded_run(harness, app_id, suite_id, &[], None)?;
    let after = assess::standing(&run);
    let decided = verdict(&after, case_id);
    let accepted = decided == "won" || decided == "tied";
    let roadmap = catalog::roadmap_of(harness, app_id, suite_id)?;
    let answer = json!({
        "appId": app_id,
        "suite": suite_id,
        "case": case_id,
        "request": request_file,
        "pursuit": pursuit,
        "acceptance": {"runId": run["runId"], "file": run_file, "verdict": decided, "standing": after},
        "accepted": accepted,
        "roadmap": roadmap,
    });
    std::io::Write::write_all(
        &mut create_private(&directory.join(format!("{request_id}.outcome.json")))?,
        serde_json::to_string_pretty(&answer)?.as_bytes(),
    )?;
    print_json(&answer)?;
    if accepted {
        return Ok(());
    }
    Err(Failure::new(
        POINT,
        Code::Refused,
        format!(
            "Jeden's pursuit {request_id} reported success, but run {}, recorded after it, still shows case {case_id} {decided}; the roadmap item stays",
            run["runId"].as_str().unwrap_or_default()
        ),
    ))
}

/// The verdict a standing gives one case, or `absent` when no contender ran it.
fn verdict(standing: &Json, case_id: &str) -> String {
    standing["cases"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entry| entry["case"] == case_id)
        .and_then(|entry| entry["verdict"].as_str())
        .unwrap_or("absent")
        .to_string()
}

/// The finish line Jeden is given: what the case asks, who beat us and by how
/// much, what may not change, and the run that decides it.
fn objective(
    app_id: &str,
    suite_id: &str,
    run_id: &str,
    ours: &str,
    file: &Path,
    case: &suite::Case,
    loss: &Json,
) -> Result<String, Failure> {
    let winners: Vec<&str> = loss["winners"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
        .collect();
    let measured = |value: &Json| {
        format!(
            "pass rate {} and p50 {} ms",
            value["passRate"].as_f64().unwrap_or(0.0),
            value["p50Ms"]
                .as_u64()
                .map_or("unmeasured".to_string(), |ms| ms.to_string())
        )
    };
    Ok(format!(
        "Make {ours} win benchmark case {suite_id}/{case_id} of {app_id}, which {winners} won.\n\n\
         The case asks: {instruction}\nIts input: {input}\nIt is decided by these assertions on the contender's result document: {assertions}\n\n\
         In run {run_id}, {winners} had {winner}; {ours} had {mine}.\n\n\
         Change {ours} in this checkout, and deliver the change to wherever our contender runs it, so that it passes this case at least as reliably and as fast as the best rival. \
         Do not change the suite file {suite_file} (runs are compared only on the same suite hash) or any rival's driver.\n\n\
         Done means: `probierz benchmark run {app_id} --suite {suite_id}` records a new run in which `probierz benchmark standing {app_id} --suite {suite_id}` shows {ours} winning or tying case {case_id}. \
         Cite that run id as the evidence. Probierz records its own run after this pursuit, and only that run closes the case.",
        case_id = case.id,
        winners = winners.join(", "),
        instruction = case.instruction,
        input = serde_json::to_string(&case.input)?,
        assertions = serde_json::to_string(&case.assertions)?,
        winner = measured(&loss["winner"]),
        mine = measured(&loss["ours"]),
        suite_file = file.display(),
    ))
}

/// A request or initiative id in the characters Jeden accepts: ASCII letters,
/// digits, `-` and `_`. Its length is Jeden's to judge: a name Jeden finds too
/// long is refused by Jeden with its own sentence, never cut here.
fn identifier(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect()
}

/// Submit the request to Jeden and read its response document. A request
/// Jeden already finished answers its stored response.
fn jeden(request: &Path) -> Result<Json, Failure> {
    let output = Command::new(JEDEN_BIN)
        .arg("pursue")
        .arg("--request-file")
        .arg(request)
        .args(["--allow-write", "--allow-command", "--json"])
        .output()
        .map_err(|error| {
            Failure::new(
                POINT,
                Code::Prerequisite,
                format!("{JEDEN_BIN} could not start ({error}); `stado product install jeden --surface cli` installs it"),
            )
        })?;
    serde_json::from_slice(&output.stdout).map_err(|_| {
        Failure::new(
            POINT,
            Code::Unavailable,
            format!(
                "{JEDEN_BIN} pursue exited {} without a response document: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        )
    })
}
