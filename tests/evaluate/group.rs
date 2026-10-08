//! `probierz evaluate` through the real binary: figure and seo are leaves of
//! one group, the retired hyphenated spellings are unknown commands, and a
//! leaf missing a required input is refused before anything is rendered,
//! crawled or asked of a model.

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
fn evaluate_lists_figure_and_seo() {
    let help = probierz(&["evaluate", "--help"]);
    assert!(help.status.success(), "{}", text(&help.stderr));
    let listing = text(&help.stdout);
    assert!(listing.contains("figure"), "{listing}");
    assert!(listing.contains("seo"), "{listing}");
}

#[test]
fn the_retired_spellings_are_unknown_commands() {
    for retired in "figure-evaluate seo-evaluate".split(' ') {
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
fn a_leaf_missing_a_required_input_is_refused() {
    let figure = probierz(&["evaluate", "figure", "--reference", "a.svg"]);
    assert!(!figure.status.success(), "{}", text(&figure.stdout));
    assert!(
        text(&figure.stderr).contains("--candidate"),
        "{}",
        text(&figure.stderr)
    );

    let seo = probierz(&["evaluate", "seo", "--app", "demo", "--mode", "release"]);
    assert!(!seo.status.success(), "{}", text(&seo.stdout));
    assert!(
        text(&seo.stderr).contains("--base-url"),
        "{}",
        text(&seo.stderr)
    );
}
