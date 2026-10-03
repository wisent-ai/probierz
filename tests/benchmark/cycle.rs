//! `probierz benchmark cycle` through the real binary acts only under a
//! written policy: without one, or with one that grants nothing it can act
//! on, it refuses before it reads Trends, the catalog or the model router.
//! Each test has a harness of its own under Cargo's target directory.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");

fn harness(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("benchmark-cycle").join(name);
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove the previous run's harness");
    }
    std::fs::create_dir_all(root.join("apps")).expect("create the harness");
    root
}

fn cycle(root: &Path) -> Output {
    Command::new(PROBIERZ)
        .args(["benchmark", "cycle"])
        .env("PROBIERZ_HARNESS_DIR", root)
        .output()
        .expect("start probierz")
}

#[test]
fn cycle_refuses_without_a_written_policy_and_records_nothing() {
    let root = harness("no-policy");

    let output = cycle(&root);

    assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("the loop acts only under a written policy"), "{stderr}");
    assert!(stderr.contains("pursuitBudgetUsd"), "{stderr}");
    assert!(!root.join("test-results").join(".autonomy").exists(), "a refused cycle recorded a report");
}

#[test]
fn cycle_refuses_a_policy_whose_pursuit_budget_is_not_positive() {
    let root = harness("zero-budget");
    std::fs::write(
        root.join("autonomy.yaml"),
        "schemaVersion: 1\nowner: wisent-ai\nproductsPerWeek: 1\nobservations: 40\nrounds: 3\ncases: 5\npursuitBudgetUsd: \"0\"\npursuitsPerProduct: 1\n",
    )
    .expect("write the policy");

    let output = cycle(&root);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is not a positive amount of US dollars"), "{stderr}");
    assert!(!root.join("test-results").join(".autonomy").exists(), "a refused cycle recorded a report");
}

#[test]
fn cycle_refuses_a_policy_with_a_key_it_does_not_know() {
    let root = harness("unknown-key");
    std::fs::write(
        root.join("autonomy.yaml"),
        "schemaVersion: 1\nowner: wisent-ai\nproductsPerWeek: 1\nobservations: 40\nrounds: 3\ncases: 5\npursuitBudgetUsd: \"5\"\npursuitsPerProduct: 1\nproductsPerDay: 9\n",
    )
    .expect("write the policy");

    let output = cycle(&root);

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not a policy"));
}
