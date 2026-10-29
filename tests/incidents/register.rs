//! The incident register is read by folding its records in the order they
//! were written. A line the fold cannot apply is a damaged register: it is
//! refused naming the line, not skipped, so an incident is never listed in a
//! state its records do not support. Through the real binary on a harness
//! made for the case under the package's build directory.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

const ACTOR: &str = "incident-register-case";
/// When a hand-written follow-up says it happened; the fold never reads it.
const WRITTEN_AT: &str = "written by the case";

fn harness(case: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("incident-register-{case}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("the previous run's harness is removed");
    }
    std::fs::create_dir_all(&root).expect("the case's harness is made");
    root
}

fn probierz(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root)
        .arg("incident")
        .args(args)
        .env("PROBIERZ_ACTOR", ACTOR)
        .output()
        .expect("start probierz")
}

/// Both streams, so a refusal is found wherever the binary prints it.
fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn register(root: &Path) -> PathBuf {
    root.join("test-results/.incidents/register.jsonl")
}

/// Append `record` to the register as the next line, as a damaged writer would.
fn append(root: &Path, record: &Value) {
    let mut text = match std::fs::read_to_string(register(root)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => panic!("{} cannot be read: {error}", register(root).display()),
    };
    text.push_str(&format!("{record}\n"));
    std::fs::create_dir_all(root.join("test-results/.incidents"))
        .expect("the register's directory is made");
    std::fs::write(register(root), text).expect("the register is written");
}

fn resolution(id: &str, note: &str) -> Value {
    json!({ "schema": "ai.wisent.probierz.incident-resolution.v1", "incident_id": id,
            "resolved_at": WRITTEN_AT, "actor": ACTOR, "note": note })
}

fn recorded(root: &Path) -> String {
    let output = probierz(
        root,
        &[
            "record",
            "--claim",
            "the case's claim",
            "--service",
            "probierz",
            "--failure-point",
            "incident.case",
            "--code",
            "case_failure",
            "--detail",
            "the case's detail",
            "--json",
        ],
    );
    assert!(output.status.success(), "{}", said(&output));
    let entry: Value =
        serde_json::from_slice(&output.stdout).expect("the record prints its entry as JSON");
    let Some(id) = entry["incident_id"].as_str() else {
        panic!("the recorded incident names no incident_id: {entry}");
    };
    id.to_string()
}

#[test]
fn a_resolution_of_an_incident_the_register_does_not_hold_is_refused_naming_its_line() {
    let root = harness("orphan");
    append(&root, &resolution("no-such-incident", "closed"));
    let output = probierz(&root, &["list", "--state", "all", "--json"]);
    assert!(!output.status.success(), "{}", said(&output));
    assert!(
        said(&output).contains("register.jsonl:"),
        "{}",
        said(&output)
    );
    assert!(
        said(&output).contains("which no earlier line records"),
        "{}",
        said(&output)
    );
    std::fs::remove_dir_all(&root).expect("the case's harness is removed");
}

#[test]
fn a_second_resolution_is_refused_instead_of_skipped() {
    let root = harness("twice");
    let id = recorded(&root);
    let resolved = probierz(&root, &["resolve", &id, "--note", "fixed"]);
    assert!(resolved.status.success(), "{}", said(&resolved));
    append(&root, &resolution(&id, "again"));
    let output = probierz(&root, &["show", &id, "--json"]);
    assert!(!output.status.success(), "{}", said(&output));
    assert!(
        said(&output).contains("which is already resolved"),
        "{}",
        said(&output)
    );
    std::fs::remove_dir_all(&root).expect("the case's harness is removed");
}

#[test]
fn a_state_the_list_does_not_know_is_refused_naming_it() {
    let root = harness("state");
    recorded(&root);
    let output = probierz(&root, &["list", "--state", "closed"]);
    assert!(!output.status.success(), "{}", said(&output));
    assert!(
        said(&output).contains("--state is open, resolved or all, not closed"),
        "{}",
        said(&output)
    );
    let listed = probierz(&root, &["list", "--state", "open", "--json"]);
    assert!(listed.status.success(), "{}", said(&listed));
    let answer: Value = serde_json::from_slice(&listed.stdout).expect("the list prints JSON");
    let Some(incidents) = answer["incidents"].as_array() else {
        panic!("the list answers no incidents array: {answer}");
    };
    assert!(!incidents.is_empty(), "{answer}");
    assert!(
        incidents.iter().all(|incident| incident["state"] == "open"),
        "{answer}"
    );
    std::fs::remove_dir_all(&root).expect("the case's harness is removed");
}
