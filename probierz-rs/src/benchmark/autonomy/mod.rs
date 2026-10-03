//! The benchmark loop without the operator. One `cycle` does, under the
//! policy he wrote once, what he otherwise decides by hand each time: which
//! markets Trends watches, which scouted products are built, which suites
//! run, which losses are pursued and with how much money, and which failures
//! of our products become their work. `schedule` has Stado run the cycle on
//! a cron, on one host, so nothing waits for anyone to type it.
//!
//! Every step is the command an operator would type, run to its end through
//! this same binary, Trends or Stado, and kept in the cycle's report under
//! `test-results/.autonomy/` with what it answered or why it refused. The
//! adoptions ledger beside it records, for every product the loop created,
//! the policy's SHA-256 and the model's verdict it was created on.

mod feedback;
mod model;
mod policy;
mod steps;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value as Json};

use super::record::catalog;
use super::scout::identity;
use policy::{adopted_this_week, append};
use crate::failure::{now_iso, print_json, Answer, Failure};
use crate::stado::{shell_quote, STADO_BIN};

const POINT: &str = "benchmark.autonomy";
const TRENDS: &str = "trends";
/// The one schedule of the loop; `stado schedule edit` changes it.
const SCHEDULE_ID: &str = "probierz-autonomy";

/// Topics a cycle report shows Trends accepted.
fn watched_ok(report: &Json) -> usize {
    report["topics"]["added"]
        .as_array()
        .map_or(0, |added| added.iter().filter(|entry| entry["topic"]["ok"] == true).count())
}

/// The source kinds Trends asks by query, read from the sources it already
/// has: a new topic gets one source of each, asked with its first term.
fn query_kinds(harness: &Path) -> Vec<String> {
    let listed = steps::run(Path::new(TRENDS), &steps::args(&["source-list"]), harness);
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
fn watch(harness: &Path, products: &[Json], policy: &policy::Policy, present: &[String]) -> Vec<Json> {
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
        let mut topic = steps::args(&["topic-add", id]);
        for term in &terms {
            topic.push("--term".to_string());
            topic.push(term.clone());
        }
        let made = steps::run(trends, &topic, harness);
        let mut sources = Vec::new();
        for kind in &kinds {
            let source = format!("{id}-{kind}");
            sources.push(steps::run(
                trends,
                &steps::args(&["source-add", source.as_str(), "--kind", kind.as_str(), "--query", terms[0].as_str()]),
                harness,
            ));
        }
        added.push(json!({"product": id, "terms": terms, "topic": made, "sources": sources}));
    }
    added
}

/// Scout one topic, have the model judge the brief, and adopt it when the
/// verdict accepts it.
fn scout_one(harness: &Path, me: &Path, policy: &policy::Policy, authority: &Json, ledger: &Path, topic: &str) -> Json {
    let observations = policy.observations.to_string();
    let rounds = policy.rounds.to_string();
    let scout = steps::run(
        me,
        &steps::args(&[
            "benchmark", "scout", topic, "--owner", policy.owner.as_str(),
            "--observations", observations.as_str(), "--rounds", rounds.as_str(),
        ]),
        harness,
    );
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
        Err(failure) => return json!({"topic": topic, "scout": scout, "judge": {"refusal": failure.detail}}),
    };
    if verdict["accept"] != true {
        return json!({"topic": topic, "scout": scout, "judge": verdict});
    }
    let cases = policy.cases.to_string();
    let brief_path = brief_file.to_string_lossy().to_string();
    let adopt = steps::run(
        me,
        &steps::args(&[
            "benchmark", "adopt", brief_path.as_str(), "--allow-create",
            "--cases", cases.as_str(), "--rounds", rounds.as_str(),
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
fn measure(harness: &Path, me: &Path, policy: &policy::Policy, app: &str, suite: &str) -> Json {
    let run = steps::run(me, &steps::args(&["benchmark", "run", app, "--suite", suite]), harness);
    let roadmap = steps::run(me, &steps::args(&["benchmark", "roadmap", app, "--suite", suite]), harness);
    let standing = steps::run(me, &steps::args(&["benchmark", "standing", app, "--suite", suite]), harness);
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
                    "benchmark", "pursue", app, "--suite", suite, "--case", case,
                    "--budget-usd", policy.pursuit_budget_usd.as_str(),
                ]),
                harness,
            )
        })
        .collect();
    json!({"app": app, "suite": suite, "run": run, "roadmap": roadmap, "standing": standing, "pursued": pursued})
}

pub(crate) fn cycle(harness: &Path, policy_file: Option<&Path>) -> Answer {
    let (file, policy, digest) = policy::load(harness, policy_file)?;
    let authority = json!({"policy": file.to_string_lossy(), "sha256": digest});
    let me = std::env::current_exe()?;
    let directory = harness.join("test-results").join(".autonomy");
    std::fs::create_dir_all(&directory)?;
    let ledger = directory.join("adoptions.jsonl");
    let started = now_iso();
    let trends = Path::new(TRENDS);

    let products = catalog::products()?;
    let mut listed = steps::run(trends, &steps::args(&["topic-list"]), harness);
    // A Trends that has no state yet is given one, read from its own error code.
    let missing = serde_json::from_str::<Json>(listed["refusal"].as_str().unwrap_or_default())
        .is_ok_and(|refusal| refusal["error"]["code"] == "state_missing");
    if missing {
        steps::run(trends, &steps::args(&["init"]), harness);
        listed = steps::run(trends, &steps::args(&["topic-list"]), harness);
    }
    let present: Vec<String> = listed["answer"]["topics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|topic| topic["name"].as_str())
        .map(str::to_string)
        .collect();
    let watched = watch(harness, &products, &policy, &present);
    let ingested = steps::run(trends, &steps::args(&["ingest"]), harness);

    let relisted = steps::run(trends, &steps::args(&["topic-list"]), harness);
    let mut scouted = Vec::new();
    for topic in relisted["answer"]["topics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|topic| topic["name"].as_str())
    {
        if adopted_this_week(&ledger) >= policy.products_per_week {
            scouted.push(json!({
                "topic": topic,
                "skipped": format!("{} product(s) a week is the policy's limit, and it is reached", policy.products_per_week),
            }));
        } else if !identity(topic) {
            scouted.push(json!({"topic": topic, "skipped": "the topic name is not a Stado identity"}));
        } else {
            scouted.push(scout_one(harness, &me, &policy, &authority, &ledger, topic));
        }
    }

    let mut measured = Vec::new();
    for product in catalog::products()? {
        let Some(app) = product["benchmark"]["app"].as_str() else {
            continue;
        };
        for suite in product["benchmark"]["suites"].as_array().into_iter().flatten().filter_map(Json::as_str) {
            measured.push(measure(harness, &me, &policy, app, suite));
        }
    }

    let fed = feedback::incidents(harness, &me, &catalog::products()?);
    let adopted = scouted.iter().filter(|entry| entry["adopt"]["ok"] == true).count();
    let report = json!({
        "schema": "ai.wisent.probierz.benchmark.cycle.v1",
        "startedAt": started,
        "finishedAt": now_iso(),
        "authority": authority,
        "topics": {"listed": listed, "added": watched},
        "ingest": ingested,
        "scouted": scouted,
        "measured": measured,
        "feedback": fed,
    });
    let report_file = directory.join(format!("cycle-{}.json", chrono::Utc::now().format("%Y%m%d%H%M%S")));
    std::fs::write(&report_file, serde_json::to_string_pretty(&report)? + "\n")?;
    let count = |key: &str, ok: &dyn Fn(&Json) -> bool| report[key].as_array().map_or(0, |all| all.iter().filter(|entry| ok(entry)).count());
    let topics = report["topics"]["added"].as_array().map_or(0, Vec::len);
    let suites = report["measured"].as_array().map_or(0, Vec::len);
    let ran = count("measured", &|entry| entry["run"]["ok"] == true);
    print_json(&json!({
        "report": report_file.to_string_lossy(),
        "topicsAdded": watched_ok(&report),
        "topicsRefused": topics - watched_ok(&report),
        "scouted": count("scouted", &|entry| entry["scout"]["ok"] == true),
        "adopted": adopted,
        "suitesMeasured": ran,
        "suitesRefused": suites - ran,
        "roadmapChanges": report["feedback"]["changes"],
    }))
}

pub(crate) fn schedule(cron: &str, host: &str, harness_dir: &str, secrets: &[String], policy_file: Option<&str>) -> Answer {
    if harness_dir.trim().is_empty() {
        return Err(Failure::invalid(POINT, "--harness-dir names the Probierz harness on the host"));
    }
    let mut command = format!(
        "PROBIERZ_HARNESS_DIR={} probierz benchmark cycle",
        shell_quote(harness_dir)
    );
    if let Some(file) = policy_file {
        command.push_str(&format!(" --policy {}", shell_quote(file)));
    }
    let mut create = Command::new(STADO_BIN);
    create.args(["schedule", "create", "--id", SCHEDULE_ID, "--json", "--cron", cron, "--pinned-host", host]);
    for secret in secrets {
        create.arg("--secret-env").arg(secret);
    }
    create.arg(&command);
    let created: Json = serde_json::from_slice(&catalog::stado(&mut create)?).map_err(|error| {
        Failure::config(POINT, format!("stado schedule create answered no JSON: {error}"))
    })?;
    print_json(&json!({
        "schedule": created,
        "command": command,
        "next": format!("stado schedule show {SCHEDULE_ID} reads it; stado schedule edit {SCHEDULE_ID} changes it"),
    }))
}
