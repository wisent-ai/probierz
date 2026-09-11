//! Project adoption through the CLI: the fixtures every case builds on, and
//! one module per concern.

mod adopt;
mod conflicts;
mod onboarding;
mod refusals;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

pub(crate) fn repository(root: &Path) {
    fs::create_dir_all(root.join(".git")).expect("Git marker");
    fs::create_dir_all(root.join("apps")).expect("apps directory");
}

pub(crate) fn source_repository() -> TempDir {
    let source = tempfile::tempdir().expect("temporary source repository");
    repository(source.path());
    fs::create_dir_all(source.path().join("apps/example")).expect("app directory");
    fs::create_dir_all(source.path().join("packages/tui/tests")).expect("spec directory");
    fs::write(
        source.path().join("apps/.adoptions.json"),
        b"source-local adoption history is not a definition\n",
    )
    .expect("source-local adoption state");
    let root = serde_json::to_string(&source.path().to_string_lossy()).expect("YAML path string");
    fs::write(
        source.path().join("apps/example/probierz.yaml"),
        format!(
            "schemaVersion: 1\nappId: example\nowner: example maintainers\nrepositories:\n  - root: {root}\n    mappings: []\nsurfaces:\n  tui:\n    spec: example.spec.mjs\n    journeys: [smoke]\njourneys:\n  smoke:\n    owner: example maintainers\n    timeoutMs: 1000\n"
        ),
    )
    .expect("manifest");
    fs::write(
        source.path().join("packages/tui/tests/example.spec.mjs"),
        "describe('example', () => { it('smoke', () => {}); });\n",
    )
    .expect("journey spec");
    fs::write(
        source.path().join("packages/tui/tests/support.mjs"),
        "export const fixture = 'retained helper';\n",
    )
    .expect("journey helper");
    source
}

pub(crate) fn source_with_identical_definitions(source: &Path) -> TempDir {
    let duplicate = tempfile::tempdir().expect("second source repository");
    repository(duplicate.path());
    for relative in [
        "apps/example/probierz.yaml",
        "packages/tui/tests/example.spec.mjs",
        "packages/tui/tests/support.mjs",
    ] {
        let target = duplicate.path().join(relative);
        fs::create_dir_all(target.parent().expect("definition parent"))
            .expect("second source definition directory");
        fs::copy(source.join(relative), &target).expect("copy identical source definition");
        #[cfg(unix)]
        {
            fs::set_permissions(
                &target,
                fs::metadata(source.join(relative)).unwrap().permissions(),
            )
            .expect("copy source definition mode");
        }
    }
    duplicate
}

pub(crate) fn destination_repository() -> TempDir {
    let destination = tempfile::tempdir().expect("temporary destination repository");
    repository(destination.path());
    destination
}

pub(crate) fn run(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root)
        .args(arguments)
        .output()
        .expect("run probierz")
}

pub(crate) fn run_with_state(root: &Path, state: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root)
        .args(arguments)
        .env("XDG_STATE_HOME", state)
        .output()
        .expect("run probierz")
}

pub(crate) fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "JSON stdout: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

pub(crate) fn assert_invocation_refused(root: &Path, arguments: &[&str], sentence: &str) {
    let output = run(root, arguments);
    assert_eq!(output.status.code(), Some(2), "{arguments:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(sentence),
        "{arguments:?} did not report {sentence:?}:\n{stderr}"
    );
}
