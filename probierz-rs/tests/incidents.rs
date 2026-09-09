//! The register of attempts that did not hold, driven through the built
//! binary.
//!
//! Every case runs `CARGO_BIN_EXE_probierz` against a harness of its own in a
//! tempdir, so the operator's real `test-results/` is never touched, and reads
//! the register file back off disk rather than trusting the line the command
//! printed. The refusal sentences below were copied from live runs of this
//! binary.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::{tempdir, TempDir};

/// A harness root the register can be written under. `apps/` is what makes a
/// directory a harness, so the binary accepts it instead of resolving the
/// repository's own root.
fn harness() -> TempDir {
    let root = tempdir().expect("a temporary harness root");
    fs::create_dir_all(root.path().join("apps")).expect("an apps directory");
    root
}

fn probierz(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root)
        .args(args)
        .env("PROBIERZ_ACTOR", "test-actor")
        .env_remove("GITHUB_ACTOR")
        .output()
        .expect("the built probierz binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn lines(root: &Path) -> Vec<Value> {
    let file = root
        .join("test-results")
        .join(".incidents")
        .join("register.jsonl");
    fs::read_to_string(&file)
        .expect("the register file exists once something is recorded")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("each line is one JSON object"))
        .collect()
}

fn recorded(root: &Path, claim: &str, detail: &str) -> String {
    let output = probierz(
        root,
        &[
            "incident",
            "record",
            "--claim",
            claim,
            "--service",
            "probierz",
            "--failure-point",
            "verification.claim",
            "--code",
            "unknown",
            "--detail",
            detail,
        ],
    );
    assert!(
        output.status.success(),
        "recording answers: {}",
        stderr(&output)
    );
    stdout(&output)
        .split_whitespace()
        .nth(1)
        .expect("the command prints the identity it recorded")
        .to_string()
}

/// The record is on disk, attributed, and carries the envelope it was given.
/// The identity the command printed is the identity the file holds, so a
/// reader who has only the file can find what the operator was told about.
#[test]
fn a_recorded_incident_is_on_disk_with_its_claim_and_its_envelope() {
    let root = harness();
    let claim = "twelve areas were verified before merge";
    let detail = "the runs were performed in worktrees a hook deleted, so no revision carries them";
    let id = recorded(root.path(), claim, detail);

    let entries = lines(root.path());
    assert_eq!(entries.len(), 1, "one line per record: {entries:?}");
    let entry = &entries[0];
    assert_eq!(entry["schema"], "ai.wisent.probierz.incident.v1");
    assert_eq!(entry["incident_id"], id);
    assert_eq!(entry["claim"], claim);
    assert_eq!(entry["actor"], "test-actor");
    assert_eq!(entry["envelope"]["detail"], detail);
    assert_eq!(entry["envelope"]["service"], "probierz");
    assert_eq!(entry["envelope"]["failure_point"], "verification.claim");
    assert!(
        entry["recorded_at"]
            .as_str()
            .is_some_and(|at| at.len() > 19),
        "the record is stamped: {entry}"
    );
}

/// An open incident is what `list` answers by default, and `show` reads the
/// same record back. An empty register says where it would be rather than
/// printing nothing.
#[test]
fn the_register_lists_what_is_open_and_says_so_when_nothing_is() {
    let root = harness();
    let empty = probierz(root.path(), &["incident", "list"]);
    assert!(empty.status.success(), "{}", stderr(&empty));
    assert!(
        stdout(&empty).contains("no open incidents in")
            && stdout(&empty).contains("test-results/.incidents/register.jsonl"),
        "an empty register names itself: {}",
        stdout(&empty)
    );

    let id = recorded(root.path(), "a claim", "what did not hold");
    let listed = probierz(root.path(), &["incident", "list"]);
    assert!(
        stdout(&listed).contains(&id) && stdout(&listed).contains("open"),
        "the row carries the identity and the state: {}",
        stdout(&listed)
    );

    let shown = probierz(root.path(), &["incident", "show", &id]);
    assert!(shown.status.success(), "{}", stderr(&shown));
    assert!(
        stdout(&shown).contains("what did not hold") && stdout(&shown).contains("test-actor"),
        "show reads the record back: {}",
        stdout(&shown)
    );
}

/// Resolving appends rather than rewriting, and the state a reader sees is
/// folded from the two records. That is the reason the file is append-only: an
/// incident cannot be quietly un-recorded.
#[test]
fn resolving_appends_a_second_record_and_the_state_is_folded_from_both() {
    let root = harness();
    let id = recorded(root.path(), "a claim", "what did not hold");

    let resolved = probierz(
        root.path(),
        &[
            "incident",
            "resolve",
            &id,
            "--note",
            "rebuilt the area and reran it on main",
        ],
    );
    assert!(resolved.status.success(), "{}", stderr(&resolved));

    let entries = lines(root.path());
    assert_eq!(entries.len(), 2, "the incident is still there: {entries:?}");
    assert_eq!(entries[0]["schema"], "ai.wisent.probierz.incident.v1");
    assert_eq!(
        entries[1]["schema"],
        "ai.wisent.probierz.incident-resolution.v1"
    );
    assert_eq!(entries[1]["incident_id"], id);
    assert_eq!(entries[1]["note"], "rebuilt the area and reran it on main");

    let open = probierz(root.path(), &["incident", "list", "--json"]);
    let report: Value = serde_json::from_str(&stdout(&open)).expect("--json prints one object");
    assert_eq!(report["total"], 0, "nothing is open: {report}");

    let all = probierz(
        root.path(),
        &["incident", "list", "--state", "all", "--json"],
    );
    let report: Value = serde_json::from_str(&stdout(&all)).expect("--json prints one object");
    assert_eq!(report["incidents"][0]["state"], "resolved");
    assert_eq!(
        report["incidents"][0]["resolution"]["actor"], "test-actor",
        "the resolution is folded on: {report}"
    );
}

/// The four refusals, each with the sentence the product prints. A register
/// that accepts an unreadable record, or closes one incident twice, is not a
/// record of anything.
#[test]
fn the_register_refuses_what_it_cannot_record_or_close() {
    let root = harness();

    let no_detail = probierz(
        root.path(),
        &[
            "incident",
            "record",
            "--claim",
            "a claim",
            "--service",
            "probierz",
            "--failure-point",
            "verification.claim",
            "--code",
            "unknown",
        ],
    );
    assert!(!no_detail.status.success());
    assert!(
        stderr(&no_detail)
            .contains("the envelope is not usable: detail must be a non-empty string"),
        "the missing field is named: {}",
        stderr(&no_detail)
    );

    let bad_state = probierz(root.path(), &["incident", "list", "--state", "maybe"]);
    assert!(!bad_state.status.success());
    assert!(
        stderr(&bad_state).contains("--state is open, resolved or all, not maybe"),
        "{}",
        stderr(&bad_state)
    );

    let unknown = probierz(root.path(), &["incident", "show", "deadbeefdeadbeef"]);
    assert!(!unknown.status.success());
    assert!(
        stderr(&unknown).contains("no incident deadbeefdeadbeef in"),
        "the refusal names the register it looked in: {}",
        stderr(&unknown)
    );

    let id = recorded(root.path(), "a claim", "what did not hold");
    let first = probierz(
        root.path(),
        &["incident", "resolve", &id, "--note", "closed once"],
    );
    assert!(first.status.success(), "{}", stderr(&first));
    let again = probierz(
        root.path(),
        &["incident", "resolve", &id, "--note", "closed twice"],
    );
    assert!(!again.status.success());
    assert!(
        stderr(&again).contains(&format!("{id} was resolved at"))
            && stderr(&again).contains("by test-actor"),
        "the second attempt names who closed it and when: {}",
        stderr(&again)
    );
    assert_eq!(
        lines(root.path())
            .iter()
            .filter(|entry| entry["schema"] == "ai.wisent.probierz.incident-resolution.v1")
            .count(),
        1,
        "the refused resolution wrote nothing"
    );
}
