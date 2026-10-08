//! `probierz secrets scan` through the real binary: a directory holding no
//! credential passes, one holding a private key block is refused with its
//! finding named, the leaf without a directory is refused, and the retired
//! spelling `secret-scan` is an unknown command.
//!
//! The directories are made for the case under the package's own build
//! directory. The key block is put together when the test runs, so this file
//! carries no key text for a scanner reading the repository.

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");
const KEY_KIND: &str = "PRIVATE KEY";

fn probierz(words: &[&str]) -> Output {
    Command::new(PROBIERZ)
        .args(words)
        .output()
        .expect("start probierz")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn directory(case: &str, file: &str, body: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("secrets-{case}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove the previous directory");
    }
    std::fs::create_dir_all(&root).expect("create the scanned directory");
    std::fs::write(root.join(file), body).expect("write the scanned file");
    root
}

fn scan(root: &PathBuf) -> (Output, Value) {
    let output = probierz(&["secrets", "scan", &root.to_string_lossy()]);
    let document = serde_json::from_slice(&output.stdout).expect("secrets scan prints JSON");
    (output, document)
}

#[test]
fn a_clean_directory_passes() {
    let root = directory("clean", "notes.txt", "a log line with nothing to hide\n");
    let (output, document) = scan(&root);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(document["passed"], Value::Bool(true), "{document}");
    std::fs::remove_dir_all(&root).expect("remove the directory");
}

#[test]
fn a_private_key_block_is_refused_and_named() {
    let block = format!(
        "-----BEGIN {KEY_KIND}-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC\n-----END {KEY_KIND}-----\n"
    );
    let root = directory("key", "leaked.pem", &block);
    let (output, document) = scan(&root);
    assert!(!output.status.success(), "a key block passed: {document}");
    assert_eq!(document["passed"], Value::Bool(false), "{document}");
    assert!(
        document["findings"]
            .as_array()
            .is_some_and(|findings| findings
                .iter()
                .any(|finding| finding["file"] == "leaked.pem")),
        "{document}"
    );
    std::fs::remove_dir_all(&root).expect("remove the directory");
}

#[test]
fn the_leaf_needs_a_directory_and_the_retired_spelling_is_unknown() {
    let missing = probierz(&["secrets", "scan"]);
    assert!(!missing.status.success(), "{}", text(&missing.stdout));
    assert!(
        text(&missing.stderr).contains("secrets scan needs a directory"),
        "{}",
        text(&missing.stderr)
    );
    let retired = probierz(&["secret-scan", "--help"]);
    assert!(!retired.status.success(), "secret-scan still runs");
}
