//! `probierz overview` through the real binary and the real Tama scanner: the
//! repository scan of an application is counted from the canonical
//! `tama violations find` report, and a machine where Tama cannot be started
//! gets a refusal that names the command instead of a clean count.
//!
//! The harness is this repository; the application is Tama, whose manifest
//! declares its desktop repository as the scanned root.

use serde::Deserialize;
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");
const APPLICATION: &str = "tama";

/// The counted scan; every count is required, so a scan that answered with
/// an error instead does not decode.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct Counted {
    violations: u64,
    skipped: u64,
    errors: u64,
}

/// The refused scan: only its cause.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Refused {
    error: String,
}

fn harness() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the package lives inside the repository")
}

fn overview(path: Option<&str>) -> Output {
    let mut command = Command::new(PROBIERZ);
    command
        .arg("--harness")
        .arg(harness())
        .args(["overview", APPLICATION, "--json"]);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command.output().expect("start probierz")
}

fn scan(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "overview refused: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("overview answers JSON");
    match report.pointer("/apps/0/violations") {
        Some(scan) => scan.clone(),
        None => panic!("the overview has no scan for {APPLICATION}: {report}"),
    }
}

#[test]
fn the_scan_is_counted_from_the_canonical_tama_report() {
    let scan = scan(&overview(None));
    if let Err(error) = serde_json::from_value::<Counted>(scan.clone()) {
        panic!("the scan is not a complete count ({error}): {scan}");
    }
}

#[test]
fn a_scanner_that_cannot_start_is_refused_by_name() {
    // Tama is installed outside the system directories, so this PATH is a
    // machine without it.
    let scan = scan(&overview(Some("/usr/bin:/bin")));
    let refused = match serde_json::from_value::<Refused>(scan.clone()) {
        Ok(refused) => refused,
        Err(error) => panic!("a missing Tama was reported as a scan ({error}): {scan}"),
    };
    assert!(
        refused.error.contains("tama violations find cannot be run"),
        "{}",
        refused.error
    );
}
