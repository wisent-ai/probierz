#![cfg(unix)]
//! The Stado bridge through the CLI: the documented refusals, and remote Byk
//! built from Stado's resolved host inventory.

mod byk;
mod fixture;
mod refusals;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

pub(crate) fn run(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root)
        .args(arguments)
        .output()
        .expect("run probierz")
}

pub(crate) fn refused(root: &Path, arguments: &[&str], sentence: &str) {
    let output = run(root, arguments);
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded: {arguments:?}"
    );
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(
        stderr.contains(sentence),
        "missing exact refusal {sentence:?} in:\n{stderr}",
    );
}

pub(crate) fn harness() -> TempDir {
    let directory = tempfile::tempdir().expect("temporary harness");
    std::fs::create_dir(directory.path().join("apps")).expect("apps");
    directory
}
