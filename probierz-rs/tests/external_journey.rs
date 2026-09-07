#![cfg(unix)]
//! A journey an application owns in its own repository.
//!
//! Most journeys are functions in this crate. A product whose journey needs
//! its own tree declares an absolute path, and the runner executes that
//! program and reports it like any other journey. Seven application manifests
//! declare exactly that, two of them at absolute paths in other repositories,
//! so this is the contract those manifests depend on.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;

fn write_program(directory: &Path, name: &str, body: &str, executable: bool) -> PathBuf {
    let path = directory.join(name);
    fs::write(&path, body).expect("program written");
    if executable {
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("executable");
    }
    path
}

fn run_journey(harness: &Path, spec: &Path, artifacts: &Path) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .args([
            "--harness",
            harness.to_str().expect("UTF-8 harness"),
            "run",
            "tui",
            "--force",
            "--no-analyze",
            "--spec",
            spec.to_str().expect("UTF-8 spec"),
            &format!("PROBIERZ_ARTIFACTS={}", artifacts.display()),
        ])
        .output()
        .expect("run command");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn an_application_owned_program_runs_and_lands_in_the_canonical_report() {
    let temporary = tempdir().expect("temporary directory");
    let harness = temporary.path().join("harness");
    let artifacts = temporary.path().join("artifacts");
    fs::create_dir_all(harness.join("apps")).expect("apps directory");
    let spec = write_program(
        temporary.path(),
        "docs-publication.probierz.spec.mjs",
        "#!/bin/sh\nprintf 'ran in %s\\n' \"$PWD\" > \"$PROBIERZ_ARTIFACTS/ran.txt\"\nexit 0\n",
        true,
    );

    let (code, stdout, stderr) = run_journey(&harness, &spec, &artifacts);
    assert_eq!(code, Some(0), "stderr: {stderr}");
    let report: Value = serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("run JSON: {stdout}"));
    let row = &report["tests"][0];
    assert_eq!(row["title"], "docs-publication", "row: {row}");
    assert_eq!(row["passed"], true, "row: {row}");
    assert_eq!(row["owner"], "application", "row: {row}");
    assert_eq!(report["total"], 1);

    // The run's environment reached the program, and its working directory was
    // the artifacts directory the run allocated.
    let evidence = fs::read_to_string(artifacts.join("ran.txt")).expect("the program wrote evidence");
    assert!(
        evidence.contains(artifacts.to_str().expect("UTF-8 artifacts")),
        "evidence: {evidence}"
    );
}

#[test]
fn a_failing_application_program_fails_the_run_and_keeps_its_reason() {
    let temporary = tempdir().expect("temporary directory");
    let harness = temporary.path().join("harness");
    let artifacts = temporary.path().join("artifacts");
    fs::create_dir_all(harness.join("apps")).expect("apps directory");
    let spec = write_program(
        temporary.path(),
        "developer-id.spec.mjs",
        "#!/bin/sh\necho 'the certificate was never issued' >&2\nexit 3\n",
        true,
    );

    let (code, stdout, _) = run_journey(&harness, &spec, &artifacts);
    assert_eq!(code, Some(1), "a failed journey fails the run");
    let report: Value = serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("run JSON: {stdout}"));
    let row = &report["tests"][0];
    assert_eq!(row["title"], "developer-id");
    assert_eq!(row["status"], "failed");
    let error = row["error"].as_str().expect("a reason");
    assert!(error.contains("exited 3"), "error: {error}");
    assert!(
        error.contains("the certificate was never issued"),
        "the program's own words must survive: {error}"
    );
}

#[test]
fn a_script_that_is_not_executable_runs_through_the_interpreter_it_names() {
    let temporary = tempdir().expect("temporary directory");
    let harness = temporary.path().join("harness");
    let artifacts = temporary.path().join("artifacts");
    fs::create_dir_all(harness.join("apps")).expect("apps directory");
    let spec = write_program(
        temporary.path(),
        "owned.probierz.spec.mjs",
        "#!/usr/bin/env node\nprocess.exit(0);\n",
        false,
    );

    let (code, stdout, stderr) = run_journey(&harness, &spec, &artifacts);
    assert_eq!(code, Some(0), "stderr: {stderr}");
    let report: Value = serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("run JSON: {stdout}"));
    assert_eq!(report["tests"][0]["title"], "owned");
    assert_eq!(report["tests"][0]["passed"], true);
}

#[test]
fn a_spec_that_is_neither_a_title_nor_a_path_is_refused_by_name() {
    let temporary = tempdir().expect("temporary directory");
    let harness = temporary.path().join("harness");
    fs::create_dir_all(harness.join("apps")).expect("apps directory");

    let output = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .args([
            "--harness",
            harness.to_str().expect("UTF-8 harness"),
            "run",
            "tui",
            "--force",
            "--spec",
            "no-such-journey",
        ])
        .output()
        .expect("run command");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "no-such-journey is neither a registered journey title nor an absolute path"
        ),
        "stderr: {stderr}"
    );
}

#[test]
fn a_declared_program_that_does_not_exist_says_so() {
    let temporary = tempdir().expect("temporary directory");
    let harness = temporary.path().join("harness");
    fs::create_dir_all(harness.join("apps")).expect("apps directory");
    let missing = temporary.path().join("never-written.spec.mjs");

    let output = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .args([
            "--harness",
            harness.to_str().expect("UTF-8 harness"),
            "run",
            "tui",
            "--force",
            "--spec",
            missing.to_str().expect("UTF-8 path"),
        ])
        .output()
        .expect("run command");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot be read"), "stderr: {stderr}");
}
