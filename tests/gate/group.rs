//! `probierz gate` through the real binary: the eight gate verbs are one
//! group, the retired hyphenated spellings are unknown commands, the hidden
//! spelling hooks installed before the group still run is accepted, and
//! `gate uninstall` removes the hook a pre-group `gate install` wrote and
//! refuses one it did not write. Each test has a repository of its own under
//! Cargo's target directory.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");
const MANAGED_MARKER: &str = "# managed-by: probierz-prepush-gate";

fn repository(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("gate-group")
        .join(name);
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove the previous run's repository");
    }
    std::fs::create_dir_all(root.join(".git").join("hooks")).expect("create the hooks folder");
    root
}

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
fn gate_lists_its_leaves_and_refuses_the_retired_spellings() {
    let help = probierz(&["gate", "--help"]);
    assert!(help.status.success(), "{}", text(&help.stderr));
    let listing = text(&help.stdout);
    for leaf in "status prepush install uninstall evaluate enforce activate deactivate".split(' ') {
        assert!(listing.contains(leaf), "gate --help does not name {leaf}: {listing}");
    }
    for retired in "gate-status gate-install gate-uninstall gate-evaluate gate-enforce gate-activate gate-deactivate".split(' ') {
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
fn the_spelling_installed_hooks_run_is_still_accepted() {
    let help = probierz(&["gate-prepush", "--help"]);
    assert!(help.status.success(), "{}", text(&help.stderr));
}

#[test]
fn gate_uninstall_removes_a_hook_gate_install_wrote_before_the_group() {
    let root = repository("managed");
    let hook = root.join(".git").join("hooks").join("pre-push");
    std::fs::write(
        &hook,
        format!("#!/bin/sh\n{MANAGED_MARKER}\nexec probierz gate-prepush --hook --app demo\n"),
    )
    .expect("write the old managed hook");

    let output = probierz(&["gate", "uninstall", "demo", "--repo", &root.to_string_lossy()]);

    assert!(output.status.success(), "{}", text(&output.stderr));
    let answer: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("gate uninstall answers JSON");
    assert_eq!(answer["removed"], serde_json::Value::Bool(true), "{answer}");
    assert!(!hook.exists(), "the managed hook is still there");
}

#[test]
fn gate_uninstall_refuses_a_hook_it_did_not_write() {
    let root = repository("foreign");
    let hook = root.join(".git").join("hooks").join("pre-push");
    std::fs::write(&hook, "#!/bin/sh\nexec ./lint\n").expect("write a foreign hook");

    let output = probierz(&["gate", "uninstall", "demo", "--repo", &root.to_string_lossy()]);

    assert!(!output.status.success(), "a foreign hook was removed");
    assert!(
        text(&output.stderr).contains("is not the hook gate install wrote"),
        "{}",
        text(&output.stderr)
    );
    assert!(hook.exists(), "the foreign hook is gone");
}
