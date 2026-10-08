//! `probierz identity` and `probierz gif` through the real binary: both
//! answer their own help, `gif` refuses a video that does not exist by name
//! and writes nothing, `identity` without an application is refused, and the
//! retired spellings `source-identity` and `readme-gif` are unknown commands.
//!
//! The output path is made under the package's own build directory.

use std::path::PathBuf;
use std::process::{Command, Output};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");

fn probierz(words: &[&str]) -> Output {
    Command::new(PROBIERZ)
        .args(words)
        .output()
        .expect("start probierz")
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn both_commands_answer_their_help_and_the_retired_spellings_are_unknown() {
    for command in ["identity", "gif"] {
        let help = probierz(&[command, "--help"]);
        assert!(help.status.success(), "{command} --help: {}", said(&help));
    }
    for retired in ["source-identity", "readme-gif"] {
        let output = probierz(&[retired, "--help"]);
        assert!(
            !output.status.success(),
            "{retired} still runs: {}",
            said(&output)
        );
    }
}

#[test]
fn gif_refuses_a_missing_video_and_writes_nothing() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("inspect-gif-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove the previous directory");
    }
    std::fs::create_dir_all(&root).expect("create the output directory");
    let video = root.join("absent.webm");
    let gif = root.join("demo.gif");
    let output = probierz(&[
        "gif",
        &video.to_string_lossy(),
        "--out",
        &gif.to_string_lossy(),
    ]);
    assert!(
        !output.status.success(),
        "a missing video rendered: {}",
        said(&output)
    );
    assert!(
        said(&output).contains("does not exist"),
        "{}",
        said(&output)
    );
    assert!(!gif.exists(), "a refused render left {}", gif.display());
    std::fs::remove_dir_all(&root).expect("remove the output directory");
}

#[test]
fn identity_needs_an_application() {
    let output = probierz(&["identity"]);
    assert!(!output.status.success(), "{}", said(&output));
    assert!(said(&output).contains("APP_ID"), "{}", said(&output));
}
