//! `probierz analyze <report> <artifacts>` through the real binary: the
//! timeline it writes hides what the run's recorded redaction names, and
//! nothing else, while a run that recorded no redaction (no manifest
//! described it) hides every named value.
//!
//! The harness and the run's artifacts are directories made for the case
//! under the package's own build directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");
const LOG_LINE: &str = "2026-10-12T10:00:00.000Z API_TOKEN=abc123 status=ok";

fn root(case: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("redaction-{case}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove the previous case");
    }
    std::fs::create_dir_all(root.join("apps")).expect("create the harness apps directory");
    root
}

/// One run's artifacts: a canonical report, the run manifest `manifest`,
/// and a stdout log holding one assignment the redaction names and one it
/// does not.
fn artifacts(root: &Path, manifest: Value) -> PathBuf {
    let artifacts = root.join("run");
    std::fs::create_dir_all(&artifacts).expect("create the artifacts directory");
    std::fs::write(
        artifacts.join("report.json"),
        json!({ "probierz": { "runId": "redaction-run" }, "tests": [] }).to_string(),
    )
    .expect("write the report");
    std::fs::write(artifacts.join("run-manifest.json"), manifest.to_string())
        .expect("write the run manifest");
    std::fs::write(artifacts.join("stdout.log"), format!("{LOG_LINE}\n"))
        .expect("write the run log");
    artifacts
}

/// The messages of the log events `analyze` wrote into the timeline.
fn analyzed_log(root: &Path, artifacts: &Path) -> Vec<String> {
    let report = artifacts.join("report.json");
    let output = Command::new(PROBIERZ)
        .env("PROBIERZ_HARNESS_DIR", root)
        .arg("analyze")
        .arg(&report)
        .arg(artifacts)
        .output()
        .expect("start probierz");
    assert!(
        output.status.success(),
        "analyze failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let timeline: Value = serde_json::from_slice(
        &std::fs::read(artifacts.join("timeline.json")).expect("read the timeline"),
    )
    .expect("the timeline is JSON");
    timeline["events"]
        .as_array()
        .expect("timeline events")
        .iter()
        .filter(|event| event["type"] == "log")
        .map(|event| event["message"].as_str().expect("log message").to_string())
        .collect()
}

#[test]
fn a_declared_redaction_hides_only_the_names_it_declares() {
    let root = root("declared");
    let artifacts = artifacts(&root, json!({ "redaction": ["token"] }));
    assert_eq!(
        analyzed_log(&root, &artifacts),
        vec!["API_TOKEN=[REDACTED] status=ok".to_string()]
    );
}

#[test]
fn a_run_no_manifest_described_hides_every_named_value() {
    let root = root("undeclared");
    let artifacts = artifacts(&root, json!({ "redaction": null }));
    assert_eq!(
        analyzed_log(&root, &artifacts),
        vec!["API_TOKEN=[REDACTED] status=[REDACTED]".to_string()]
    );
}
