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

mod cycle;
mod feedback;
mod model;
mod policy;
mod steps;

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value as Json};

use super::record::catalog;
use super::scout::identity;
use crate::failure::{now_iso, print_json, Answer, Failure};
use crate::stado::{shell_quote, STADO_BIN};
use cycle::topics::{evidence_topics, topic_names, watch};
use cycle::work::{measure, scout_one};
use policy::adopted_this_week;
use sha2::Digest as _;

const POINT: &str = "benchmark.autonomy";
const TRENDS: &str = "trends";

/// Topics a cycle report shows Trends accepted.
fn watched_ok(report: &Json) -> usize {
    report["topics"]["added"].as_array().map_or(0, |added| {
        added
            .iter()
            .filter(|entry| entry["topic"]["ok"] == true)
            .count()
    })
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
    // Without Trends and Competitors the policy's evidence directory is the
    // list of topics, one file each; nothing is watched or ingested.
    let (listed, watched, ingested, topics) = match &policy.evidence_dir {
        Some(directory) => {
            let topics = evidence_topics(directory)?;
            let listed = json!({"evidenceDir": directory.to_string_lossy(), "topics": topics});
            (
                listed,
                Vec::new(),
                json!({"skipped": "the policy names an evidence directory"}),
                topics,
            )
        }
        None => {
            let mut listed = steps::run(trends, &steps::command("topic list"), harness);
            // A Trends that has no state yet is given one, read from its own error code.
            let missing =
                serde_json::from_str::<Json>(listed["refusal"].as_str().unwrap_or_default())
                    .is_ok_and(|refusal| refusal["error"]["code"] == "state_missing");
            if missing {
                steps::run(trends, &steps::args(&["init"]), harness);
                listed = steps::run(trends, &steps::command("topic list"), harness);
            }
            let present: Vec<String> = topic_names(&listed["answer"]);
            let watched = watch(harness, &products, &policy, &present);
            let ingested = steps::run(trends, &steps::args(&["ingest"]), harness);
            let relisted = steps::run(trends, &steps::command("topic list"), harness);
            let topics = topic_names(&relisted["answer"]);
            (listed, watched, ingested, topics)
        }
    };
    let mut scouted = Vec::new();
    for topic in &topics {
        if adopted_this_week(&ledger)? >= policy.products_per_week {
            scouted.push(json!({
                "topic": topic,
                "skipped": format!("{} product(s) a week is the policy's limit, and it is reached", policy.products_per_week),
            }));
        } else if !identity(topic) {
            scouted
                .push(json!({"topic": topic, "skipped": "the topic name is not a Stado identity"}));
        } else {
            scouted.push(scout_one(harness, &me, &policy, &authority, &ledger, topic));
        }
    }

    let mut measured = Vec::new();
    for product in catalog::products()? {
        let Some(app) = product["benchmark"]["app"].as_str() else {
            continue;
        };
        for suite in product["benchmark"]["suites"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
        {
            measured.push(measure(harness, &me, &policy, app, suite));
        }
    }

    let fed = feedback::incidents(harness, &me, &catalog::products()?);
    let adopted = scouted
        .iter()
        .filter(|entry| entry["adopt"]["ok"] == true)
        .count();
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
    let report_file = directory.join(format!(
        "cycle-{}.json",
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    ));
    std::fs::write(&report_file, serde_json::to_string_pretty(&report)? + "\n")?;
    let count = |key: &str, ok: &dyn Fn(&Json) -> bool| {
        report[key]
            .as_array()
            .map_or(0, |all| all.iter().filter(|entry| ok(entry)).count())
    };
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

pub(crate) fn schedule(
    cron: &str,
    host: &str,
    harness_dir: &str,
    secrets: &[String],
    settings: &[String],
    policy_file: Option<&str>,
) -> Answer {
    if harness_dir.trim().is_empty() {
        return Err(Failure::invalid(
            POINT,
            "--harness-dir names the Probierz harness on the host",
        ));
    }
    let mut command = format!("PROBIERZ_HARNESS_DIR={}", shell_quote(harness_dir));
    for setting in settings {
        let named = setting.split_once('=').filter(|(name, _)| {
            name.chars()
                .next()
                .is_some_and(|first| first.is_ascii_uppercase() || first == '_')
                && name
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        });
        let Some((name, value)) = named else {
            return Err(Failure::invalid(
                POINT,
                format!("--env {setting:?} is not NAME=VALUE with an upper-case environment name"),
            ));
        };
        command.push_str(&format!(" {name}={}", shell_quote(value)));
    }
    command.push_str(" probierz benchmark cycle");
    if let Some(file) = policy_file {
        command.push_str(&format!(" --policy {}", shell_quote(file)));
    }
    // A job pinned to a host but not to its provider waits in the queue for a
    // machine type no fleet host offers; the registry names the host's kind,
    // which is the provider its agent claims for.
    let mut kind = Command::new(STADO_BIN);
    kind.args([
        "registry",
        "pull",
        "--path",
        &format!("targets.{host}.kind"),
    ]);
    let provider = String::from_utf8_lossy(&catalog::stado(&mut kind)?)
        .trim()
        .to_string();
    // The creation identity is the declaration's own digest: the same inputs
    // are the same schedule, so running this again changes nothing, and Stado
    // keeps a removed schedule's identity, so new inputs need a new one.
    let declaration = json!({"cron": cron, "host": host, "provider": provider, "secrets": secrets, "command": command});
    let digest = sha2::Sha256::digest(declaration.to_string().as_bytes());
    let hex: String = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let id = format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    );
    let mut list = Command::new(STADO_BIN);
    list.args(["schedule", "list", "--json"]);
    let schedules: Json = serde_json::from_slice(&catalog::stado(&mut list)?).map_err(|error| {
        Failure::config(
            POINT,
            format!("stado schedule list answered no JSON: {error}"),
        )
    })?;
    let ours = format!("sch-{}", hex);
    let others: Vec<String> = schedules
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["deleted"] != true && row["schedule_id"] != ours.as_str())
        .filter(|row| {
            row["command"]
                .as_str()
                .is_some_and(|line| line.contains(" probierz benchmark cycle"))
        })
        .filter_map(|row| row["schedule_id"].as_str().map(str::to_string))
        .collect();
    if !others.is_empty() {
        return Err(Failure::invalid(
            POINT,
            format!(
                "the loop already runs under schedule(s) {}; one loop runs at a time, so stado schedule rm removes the old one first",
                others.join(", ")
            ),
        ));
    }
    let mut create = Command::new(STADO_BIN);
    create.args([
        "schedule",
        "create",
        "--id",
        id.as_str(),
        "--json",
        "--cron",
        cron,
        "--pinned-host",
        host,
    ]);
    create.args(["--provider", provider.as_str(), "--pin-provider"]);
    for secret in secrets {
        create.arg("--secret-env").arg(secret);
    }
    create.arg(&command);
    let created: Json = serde_json::from_slice(&catalog::stado(&mut create)?).map_err(|error| {
        Failure::config(
            POINT,
            format!("stado schedule create answered no JSON: {error}"),
        )
    })?;
    print_json(&json!({
        "schedule": created,
        "command": command,
        "next": format!("stado schedule show {ours} reads it; stado schedule run {ours} --retry-token <TOKEN> runs it now; stado schedule edit {ours} changes it"),
    }))
}
