//! Probierz's declared refusals through the real binary: `probierz refusals
//! check` holds every cause to its test, `list` and `show` print them, and
//! the causes no other test provokes are provoked here, each asserting the
//! failure point, the code, the exit status and that nothing was written.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");

/// The checkout this test belongs to: the harness every test path is under.
fn checkout() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the package sits in the checkout")
        .to_path_buf()
}

fn probierz(args: &[&str]) -> Output {
    Command::new(PROBIERZ)
        .args(args)
        .output()
        .expect("start probierz")
}

/// The structured failure line Probierz writes on stderr.
fn failure(output: &Output) -> Value {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr
        .lines()
        .find_map(|line| line.strip_prefix("probierz-failure "))
        .unwrap_or_else(|| panic!("no probierz-failure line: {stderr}"));
    serde_json::from_str(line).expect("the failure line is JSON")
}

/// A directory of this case under the package's build directory.
fn case(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("errors-{name}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove the previous case");
    }
    std::fs::create_dir_all(&root).expect("create the case");
    root
}

#[test]
fn every_declared_refusal_names_a_test_this_checkout_holds() {
    let root = checkout();
    let output = probierz(&[
        "--harness",
        &root.to_string_lossy(),
        "refusals",
        "check",
        "--root",
        &root.to_string_lossy(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let answer: Value = serde_json::from_slice(&output.stdout).expect("check answers JSON");
    assert_eq!(answer["problems"], serde_json::json!([]), "{answer}");

    let shown = probierz(&["--harness", &root.to_string_lossy(), "refusals", "show", "probierz"]);
    assert!(shown.status.success(), "{}", String::from_utf8_lossy(&shown.stderr));
    let operation: Value = serde_json::from_slice(&shown.stdout).expect("show answers JSON");
    let causes = operation["causes"].as_array().expect("the operation lists its causes");
    assert_eq!(causes.len(), answer["causes"].as_u64().unwrap() as usize, "{operation}");
    for cause in causes {
        let retryable = cause["retryable"].as_bool().expect("a cause says whether it is retryable");
        assert_eq!(cause["exit"], if retryable { 69 } else { 1 }, "{cause}");
    }

    let unknown = probierz(&["--harness", &root.to_string_lossy(), "refusals", "show", "nothing"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert_eq!(failure(&unknown)["failure_point"], "refusals.show");
}

#[test]
fn a_check_of_a_checkout_without_the_tests_is_refused_naming_each_cause() {
    let empty = case("check-empty");
    std::fs::create_dir_all(empty.join("apps")).expect("make the harness");
    let output = probierz(&[
        "--harness",
        &empty.to_string_lossy(),
        "refusals",
        "check",
    ]);
    assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    let refused = failure(&output);
    assert_eq!(refused["failure_point"], "refusals.check");
    let detail = refused["detail"].as_str().expect("a detail");
    assert!(detail.contains("cause harness-unresolved"), "{detail}");
    assert!(detail.contains("cannot be read"), "{detail}");
    std::fs::remove_dir_all(&empty).expect("remove the case");
}

#[test]
fn a_harness_without_apps_is_refused_naming_where_it_looked() {
    let bare = case("bare-harness");
    let output = probierz(&["--harness", &bare.to_string_lossy(), "status", "demo"]);
    assert_eq!(output.status.code(), Some(1));
    let refused = failure(&output);
    assert_eq!(refused["failure_point"], "harness.resolve");
    assert_eq!(refused["error_code"], "config");
    assert_eq!(refused["retryable"], false);
    assert!(
        refused["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains(&*bare.to_string_lossy())),
        "{refused}"
    );
    std::fs::remove_dir_all(&bare).expect("remove the case");
}

#[test]
fn an_incident_without_a_claim_is_refused_and_nothing_is_written() {
    let harness = case("incident-claim");
    std::fs::create_dir_all(harness.join("apps")).expect("make the harness");
    let output = probierz(&[
        "--harness",
        &harness.to_string_lossy(),
        "incident",
        "record",
        "--claim",
        " ",
        "--service",
        "demo",
        "--failure-point",
        "demo.step",
        "--code",
        "refused",
        "--detail",
        "the step refused",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let refused = failure(&output);
    assert_eq!(refused["failure_point"], "incident.record");
    assert_eq!(refused["error_code"], "invalid");
    assert!(
        refused["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("--claim")),
        "{refused}"
    );
    let written: Vec<_> = walk(&harness)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "jsonl"))
        .collect();
    assert!(written.is_empty(), "a refused record wrote {written:?}");
    std::fs::remove_dir_all(&harness).expect("remove the case");
}

/// Every file under `root`.
fn walk(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root).expect("read the case").flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}
