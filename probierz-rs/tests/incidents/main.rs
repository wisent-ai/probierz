//! Real CLI register stories; all state is isolated under the build directory.
mod http;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn harness() -> TempDir {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/incident-tests");
    fs::create_dir_all(&directory).unwrap();
    let root = tempfile::tempdir_in(directory).unwrap();
    fs::create_dir(root.path().join("apps")).unwrap();
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
        .expect("run product")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
fn contents(root: &Path) -> String {
    fs::read_to_string(root.join("test-results/.incidents/register.jsonl")).unwrap()
}
fn lines(root: &Path) -> Vec<Value> {
    contents(root)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn recorded(root: &Path) -> String {
    let output = probierz(
        root,
        &[
            "incident",
            "record",
            "--claim",
            "Verified before merge",
            "--service",
            "probierz",
            "--failure-point",
            "verification.claim",
            "--code",
            "unknown",
            "--detail",
            "No retained run existed",
            "--json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["incident_id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn recording_survives_reading_and_refuses_an_ambiguous_envelope() {
    let root = harness();
    let envelope = root.path().join("envelope.json");
    fs::write(&envelope, r#"{"service":"probierz","failure_point":"verification.claim","error_code":"unknown","detail":"No retained run existed"}"#).unwrap();
    let both = probierz(
        root.path(),
        &[
            "incident",
            "record",
            "--claim",
            "Verified before merge",
            "--envelope",
            envelope.to_str().unwrap(),
            "--service",
            "probierz",
            "--detail",
            "Different detail",
        ],
    );
    assert!(!both.status.success());
    assert!(
        stderr(&both).contains("--service, --detail would be a second answer to the same field")
    );
    let output = probierz(
        root.path(),
        &[
            "incident",
            "record",
            "--claim",
            "Verified before merge",
            "--envelope",
            envelope.to_str().unwrap(),
            "--json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let entries = lines(root.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["claim"], "Verified before merge");
    assert_eq!(entries[0]["actor"], "test-actor");
    assert_eq!(entries[0]["envelope"]["detail"], "No retained run existed");
    let id = entries[0]["incident_id"].as_str().unwrap();
    let before = contents(root.path());
    let shown = probierz(root.path(), &["incident", "show", id, "--json"]);
    assert!(shown.status.success());
    let shown: Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(shown["claim"], entries[0]["claim"]);
    assert_eq!(shown["state"], "open");
    assert_eq!(contents(root.path()), before);
}

#[test]
fn resolution_appends_once_and_changes_open_and_resolved_queries() {
    let root = harness();
    let id = recorded(root.path());
    let before = contents(root.path());
    let resolved = probierz(
        root.path(),
        &[
            "incident",
            "resolve",
            &id,
            "--note",
            "Retained verification",
            "--run",
            "verified-run",
        ],
    );
    assert!(resolved.status.success(), "{}", stderr(&resolved));
    let after = contents(root.path());
    assert!(after.starts_with(&before));
    assert_eq!(lines(root.path()).len(), 2);
    assert_eq!(lines(root.path())[1]["run_id"], "verified-run");
    let again = probierz(
        root.path(),
        &["incident", "resolve", &id, "--note", "Second closure"],
    );
    assert!(!again.status.success());
    assert!(stderr(&again).contains(&format!("{id} was resolved at")));
    assert_eq!(contents(root.path()), after);
    let open = probierz(root.path(), &["incident", "list", "--json"]);
    assert!(open.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&open.stdout).unwrap()["total"],
        0
    );
    let resolved = probierz(
        root.path(),
        &["incident", "list", "--state", "resolved", "--json"],
    );
    assert!(resolved.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&resolved.stdout).unwrap()["incidents"][0]["incident_id"],
        id
    );
}

#[test]
fn invalid_inputs_leave_existing_register_unchanged() {
    let root = harness();
    recorded(root.path());
    let before = contents(root.path());
    let missing = probierz(
        root.path(),
        &[
            "incident",
            "record",
            "--claim",
            "A claim",
            "--service",
            "probierz",
            "--failure-point",
            "verification.claim",
            "--code",
            "unknown",
        ],
    );
    assert!(!missing.status.success());
    assert!(
        stderr(&missing).contains("the envelope is not usable: detail must be a non-empty string")
    );
    let state = probierz(root.path(), &["incident", "list", "--state", "maybe"]);
    assert!(!state.status.success());
    assert!(stderr(&state).contains("--state is open, resolved or all, not maybe"));
    let limit = probierz(root.path(), &["incident", "list", "--limit", "0"]);
    assert!(!limit.status.success());
    assert!(stderr(&limit).contains("--limit needs a positive number"));
    let unknown = probierz(root.path(), &["incident", "show", "deadbeefdeadbeef"]);
    assert!(!unknown.status.success());
    assert!(stderr(&unknown).contains("no incident deadbeefdeadbeef in"));
    assert_eq!(contents(root.path()), before);
}
