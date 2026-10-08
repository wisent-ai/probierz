//! `probierz history <app> --journey <journey>` through the real binary: the
//! newest passing run of the runs that carried the journey is
//! `summary.lastGreenRun`, whole, and `last-green`, the second command that
//! answered the same question, is an unknown command.
//!
//! The harness is a directory made for the case under the package's own
//! build directory: an `apps/` directory, as the harness root requires, and
//! three run manifests of one application on one target.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");
const APP: &str = "demo";
const TARGET: &str = "web";

fn probierz(harness: &Path, words: &[&str]) -> Output {
    Command::new(PROBIERZ)
        .env("PROBIERZ_HARNESS_DIR", harness)
        .args(words)
        .output()
        .expect("start probierz")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn run(harness: &Path, run_id: &str, status: &str, started_at: &str, journey: &str) {
    let directory = harness
        .join("test-results")
        .join(APP)
        .join(TARGET)
        .join(run_id);
    std::fs::create_dir_all(&directory).expect("create the run directory");
    let manifest = json!({
        "runId": run_id, "appId": APP, "target": TARGET, "status": status,
        "startedAt": started_at, "appManifest": { "journeys": [journey] },
    });
    std::fs::write(directory.join("run-manifest.json"), manifest.to_string())
        .expect("write the run manifest");
}

fn harness() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("history-journey-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove the previous harness");
    }
    std::fs::create_dir_all(root.join("apps")).expect("create the harness apps directory");
    run(
        &root,
        "older-sign-in-pass",
        "passed",
        "2026-10-01T10:00:00Z",
        "sign-in",
    );
    run(
        &root,
        "newer-sign-in-fail",
        "failed",
        "2026-10-02T10:00:00Z",
        "sign-in",
    );
    run(
        &root,
        "newest-checkout-pass",
        "passed",
        "2026-10-03T10:00:00Z",
        "checkout",
    );
    root
}

fn last_green(harness: &Path, extra: &[&str]) -> Value {
    let mut words = vec!["history", APP, "--target", TARGET];
    words.extend_from_slice(extra);
    let output = probierz(harness, &words);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let document: Value = serde_json::from_slice(&output.stdout).expect("history prints JSON");
    document["summary"]["lastGreenRun"]["runId"].clone()
}

#[test]
fn the_newest_passing_run_of_a_journey_is_the_last_green_run() {
    let root = harness();
    assert_eq!(
        last_green(&root, &["--journey", "sign-in"]),
        json!("older-sign-in-pass")
    );
    assert_eq!(last_green(&root, &[]), json!("newest-checkout-pass"));
    let retired = probierz(&root, &["last-green", APP]);
    assert!(!retired.status.success(), "last-green still answers");
    std::fs::remove_dir_all(&root).expect("remove the harness");
}
