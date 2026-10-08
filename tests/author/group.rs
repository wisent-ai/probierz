//! `probierz author` through the real binary: spec and manifest are leaves of
//! one group, the retired hyphenated spellings are unknown commands, and a
//! leaf missing what it needs is refused before any model is asked.

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
fn author_lists_spec_and_manifest() {
    let help = probierz(&["author", "--help"]);
    assert!(help.status.success(), "{}", text(&help.stderr));
    let listing = text(&help.stdout);
    assert!(listing.contains("spec"), "{listing}");
    assert!(listing.contains("manifest"), "{listing}");
}

#[test]
fn the_retired_spellings_are_unknown_commands() {
    for retired in "author-spec author-manifest".split(' ') {
        let refused = probierz(&[retired, "--help"]);
        assert!(!refused.status.success(), "{retired} still runs");
        assert!(
            text(&refused.stderr).contains(retired),
            "the refusal of {retired} does not name it: {}",
            text(&refused.stderr)
        );
    }
}

#[test]
fn author_spec_without_its_target_is_refused_before_any_model_is_asked() {
    let refused = probierz(&[
        "author",
        "spec",
        "demo",
        "first-use",
        "--desc",
        "x",
        "--rounds",
        "1",
    ]);
    assert!(!refused.status.success(), "{}", text(&refused.stdout));
    assert!(
        text(&refused.stderr).contains("--target"),
        "{}",
        text(&refused.stderr)
    );
}
