//! The work one cycle does per topic and per suite: scout a topic, have
//! the model judge the brief and adopt it when accepted; run a suite, bring
//! the roadmap in line and hand its losses to pursuits.

use std::path::{Path, PathBuf};

use serde_json::{json, Value as Json};

use super::super::policy::append;
use super::super::{model, policy, steps};
use crate::failure::now_iso;

/// Scout one topic, have the model judge the brief, and adopt it when the
/// verdict accepts it.
pub(in crate::benchmark::autonomy) fn scout_one(
    harness: &Path,
    me: &Path,
    policy: &policy::Policy,
    authority: &Json,
    ledger: &Path,
    topic: &str,
) -> Json {
    let observations = policy.observations.to_string();
    let rounds = policy.rounds.to_string();
    let mut words = vec![
        "benchmark",
        "scout",
        topic,
        "--owner",
        policy.owner.as_str(),
        "--observations",
        observations.as_str(),
        "--rounds",
        rounds.as_str(),
    ];
    let evidence = policy.evidence_dir.as_ref().map(|directory| {
        directory
            .join(format!("{topic}.json"))
            .to_string_lossy()
            .into_owned()
    });
    if let Some(file) = &evidence {
        words.extend(["--evidence", file.as_str()]);
    }
    let scout = steps::run(me, &steps::args(&words), harness);
    let Some(brief_file) = scout["answer"]["brief"].as_str().map(PathBuf::from) else {
        return json!({"topic": topic, "scout": scout});
    };
    let brief: Option<Json> = std::fs::read_to_string(&brief_file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let Some(brief) = brief else {
        return json!({"topic": topic, "scout": scout, "skipped": "the brief scout wrote cannot be read"});
    };
    let verdict = match model::judge(harness, &brief, policy.rounds) {
        Ok(verdict) => verdict,
        Err(failure) => {
            return json!({"topic": topic, "scout": scout, "judge": {"refusal": failure.detail}})
        }
    };
    if verdict["accept"] != true {
        return json!({"topic": topic, "scout": scout, "judge": verdict});
    }
    let cases = policy.cases.to_string();
    let brief_path = brief_file.to_string_lossy().to_string();
    let adopt = steps::run(
        me,
        &steps::args(&[
            "benchmark",
            "adopt",
            brief_path.as_str(),
            "--allow-create",
            "--cases",
            cases.as_str(),
            "--rounds",
            rounds.as_str(),
        ]),
        harness,
    );
    let mut ledger_refusal = Json::Null;
    if steps::ok(&adopt) {
        let entry = json!({
            "at": now_iso(),
            "product": adopt["answer"]["product"],
            "brief": brief_path,
            "authority": authority,
            "verdict": verdict,
        });
        if let Err(error) = append(ledger, &entry) {
            ledger_refusal = json!(format!("{}: {error}", ledger.display()));
        }
    }
    json!({"topic": topic, "scout": scout, "judge": verdict, "adopt": adopt, "ledger": ledger_refusal})
}

/// Run one suite, bring the roadmap in line with it, and hand the newest
/// losses to pursuits within the policy's budget.
pub(in crate::benchmark::autonomy) fn measure(
    harness: &Path,
    me: &Path,
    policy: &policy::Policy,
    app: &str,
    suite: &str,
) -> Json {
    let run = steps::run(
        me,
        &steps::args(&["benchmark", "run", app, "--suite", suite]),
        harness,
    );
    let roadmap = steps::run(
        me,
        &steps::args(&["benchmark", "roadmap", app, "--suite", suite]),
        harness,
    );
    let standing = steps::run(
        me,
        &steps::args(&["benchmark", "standing", app, "--suite", suite]),
        harness,
    );
    let pursued: Vec<Json> = standing["answer"]["losses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|loss| loss["case"].as_str())
        .take(policy.pursuits_per_product)
        .map(|case| {
            steps::run(
                me,
                &steps::args(&[
                    "benchmark",
                    "pursue",
                    app,
                    "--suite",
                    suite,
                    "--case",
                    case,
                    "--budget-usd",
                    policy.pursuit_budget_usd.as_str(),
                ]),
                harness,
            )
        })
        .collect();
    json!({"app": app, "suite": suite, "run": run, "roadmap": roadmap, "standing": standing, "pursued": pursued})
}
