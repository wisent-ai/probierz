//! `probierz benchmark author` through the real binary: suite and contender
//! are leaves of one group, the retired `author-suite` spelling is an
//! unknown command, and a leaf missing what it needs is refused by the
//! parser before any catalog or model is asked.

use std::process::{Command, Output};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");

fn probierz(words: &[&str]) -> Output {
    Command::new(PROBIERZ)
        .args(words)
        .output()
        .expect("start probierz")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn benchmark_author_lists_suite_and_contender() {
    let help = probierz(&["benchmark", "author", "--help"]);
    assert!(help.status.success(), "{}", text(&help.stderr));
    let listing = text(&help.stdout);
    assert!(listing.contains("suite"), "{listing}");
    assert!(listing.contains("contender"), "{listing}");
}

#[test]
fn the_retired_author_suite_is_an_unknown_command() {
    let refused = probierz(&["benchmark", "author-suite", "--help"]);
    assert!(
        !refused.status.success(),
        "benchmark author-suite still runs"
    );
    assert!(
        text(&refused.stderr).contains("author-suite"),
        "the refusal does not name the retired spelling: {}",
        text(&refused.stderr)
    );
}

#[test]
fn author_contender_without_its_contender_is_refused_by_the_parser() {
    let refused = probierz(&["benchmark", "author", "contender", "demo", "--suite", "s"]);
    assert!(!refused.status.success(), "{}", text(&refused.stdout));
    assert!(
        text(&refused.stderr).contains("--contender"),
        "{}",
        text(&refused.stderr)
    );
}
