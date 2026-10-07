//! `probierz benchmark scout` and `adopt` through the real binary, against
//! the real Trends CLI built from its sibling checkout, in a harness and a
//! Trends state of their own under Cargo's target directory, so no
//! operator's harness, catalog or Trends history is read or written.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");

/// A fresh directory for one test, with an empty `apps/` so Probierz takes
/// it as its harness.
fn harness(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("benchmark-scout")
        .join(name);
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove the previous run's harness");
    }
    std::fs::create_dir_all(root.join("apps")).expect("create the harness");
    root
}

/// The Trends binary built in the sibling checkout; the test needs the real
/// product, so its absence fails the test with what to build.
fn trends_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../trends/target/debug");
    assert!(
        dir.join("trends").is_file(),
        "{} has no trends binary: build wisent-ai/trends (cargo build) in its checkout first",
        dir.display()
    );
    dir
}

fn run(root: &Path, args: &[&str], trends_state: Option<&Path>) -> Output {
    let path = format!(
        "{}:{}",
        trends_dir().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new(PROBIERZ);
    command
        .args(args)
        .env("PROBIERZ_HARNESS_DIR", root)
        .env("PATH", path)
        .env_remove("STADO_MODEL_ROUTER_URL")
        .env_remove("STADO_MODEL_ROUTER_TOKEN");
    if let Some(state) = trends_state {
        command.env("TRENDS_STATE_FILE", state);
    }
    command.output().expect("start probierz")
}

fn trends(state: &Path, args: &[&str]) {
    let status = Command::new(trends_dir().join("trends"))
        .args(args)
        .env("TRENDS_STATE_FILE", state)
        .status()
        .expect("start trends");
    assert!(status.success(), "trends {args:?} failed");
}

#[test]
fn scout_refuses_a_topic_below_its_evidence_floor_and_writes_no_brief() {
    let root = harness("floor");
    let state = root.join("trends.state.json");
    trends(&state, &["init"]);
    trends(
        &state,
        &[
            "topic-add",
            "quiet-topic",
            "--term",
            "a term nothing published",
        ],
    );

    let output = run(
        &root,
        &["benchmark", "scout", "quiet-topic", "--owner", "wisent-ai"],
        Some(&state),
    );

    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let printed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the refusal prints the trend");
    assert_eq!(printed["trend"]["evidence"], 0);
    assert!(
        !root.join("test-results").join(".scout").exists(),
        "a refused scout wrote a brief"
    );
    assert!(
        stderr.contains("Trends reads topic quiet-topic as insufficient-evidence"),
        "{stderr}"
    );
}

#[test]
fn scout_refuses_a_topic_trends_does_not_watch() {
    let root = harness("unwatched");
    let state = root.join("trends.state.json");
    trends(&state, &["init"]);

    let output = run(
        &root,
        &["benchmark", "scout", "never-added", "--owner", "wisent-ai"],
        Some(&state),
    );

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no topic 'never-added'"), "{stderr}");
}

#[test]
fn scout_refuses_names_that_cannot_become_stado_identities() {
    let root = harness("identity");

    let output = run(
        &root,
        &[
            "benchmark",
            "scout",
            "Browser_Agents",
            "--owner",
            "wisent-ai",
        ],
        None,
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("become Stado identities"));
}

#[test]
fn adopt_refuses_without_the_operators_authority_and_creates_nothing() {
    let root = harness("authority");
    let brief = root.join("brief.json");
    std::fs::write(
        &brief,
        serde_json::json!({
            "schema": "ai.wisent.probierz.benchmark.opportunity.v1",
            "creation": {"product": {"id": "scouted"}, "repositories": [{"surface": "cli", "repository": "wisent-ai/scouted"}]},
        })
        .to_string(),
    )
    .expect("write the brief");

    let output = run(
        &root,
        &["benchmark", "adopt", brief.to_str().expect("utf-8 path")],
        None,
    );

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("wisent-ai/scouted") && stderr.contains("--allow-create"),
        "{stderr}"
    );
    assert!(
        !root.join("brief.creation.json").exists(),
        "a refused adoption wrote its creation request"
    );
}

#[test]
fn adopt_refuses_a_file_that_is_not_a_scouted_brief() {
    let root = harness("not-a-brief");
    let other = root.join("other.json");
    std::fs::write(&other, "{\"schema\": \"something else\"}").expect("write the file");

    let output = run(
        &root,
        &[
            "benchmark",
            "adopt",
            other.to_str().expect("utf-8 path"),
            "--allow-create",
        ],
        None,
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not a scouted brief"));
}
