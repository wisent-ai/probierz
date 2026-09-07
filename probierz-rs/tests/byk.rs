//! The byk-auth journey's three modes, through the real binary.
//!
//! This target logs into a real Apple account whose one-time code arrives in a
//! real mailbox, so the success path needs a provisioned mailbox broker, a
//! built application and a device. What every host can observe is the
//! product's answer when those are absent, and that answer is the contract:
//! it has to name the missing thing, not report something else.

use std::process::Command;

fn probierz() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_probierz"));
    command.current_dir(env!("CARGO_MANIFEST_DIR"));
    command
}

fn stderr_of(mut command: Command) -> (Option<i32>, String) {
    let output = command.output().expect("the product runs");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn the_two_byk_modes_are_refused_on_every_other_target() {
    for target in ["tui", "web", "mobile:ios"] {
        for flag in ["--local", "--seed-resend"] {
            let mut command = probierz();
            command.args(["run", target, flag]);
            let (code, stderr) = stderr_of(command);
            assert_eq!(code, Some(1), "{target} {flag} must refuse");
            assert!(
                stderr.contains(&format!(
                    "--local and --seed-resend apply to mobile:ios:byk-auth, not {target}"
                )),
                "{target} {flag} said: {stderr}"
            );
            assert!(
                stderr.contains("Your request was refused; nothing ran."),
                "{target} {flag} must say nothing ran: {stderr}"
            );
        }
    }
}

#[test]
fn seeding_without_a_provisioned_broker_names_the_variable_and_what_it_must_serve() {
    let mut command = probierz();
    command
        .args(["run", "mobile:ios:byk-auth", "--seed-resend"])
        .env_remove("BYK_MAILBOX_BROKER");
    let (code, stderr) = stderr_of(command);
    assert_eq!(
        code,
        Some(1),
        "seeding without a broker must refuse: {stderr}"
    );
    assert!(
        stderr.contains("BYK_MAILBOX_BROKER is required"),
        "said: {stderr}"
    );
    assert!(
        stderr.contains("mailbox-broker --mailbox byk-ios-login --socket <path>"),
        "the refusal must say what the executable has to serve: {stderr}"
    );
    assert!(
        stderr.contains("seed-resend <env-file>"),
        "the refusal must name the seeding call: {stderr}"
    );
}

#[test]
fn a_broker_that_is_not_an_executable_is_refused_by_path() {
    let directory = std::env::temp_dir().join(format!("probierz-byk-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("scratch");
    let not_executable = directory.join("not-a-broker");
    std::fs::write(&not_executable, b"#!/bin/sh\n").expect("file");

    let mut relative = probierz();
    relative
        .args(["run", "mobile:ios:byk-auth", "--seed-resend"])
        .env("BYK_MAILBOX_BROKER", "bin/broker");
    let (code, stderr) = stderr_of(relative);
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("BYK_MAILBOX_BROKER must be an absolute path, not bin/broker"),
        "said: {stderr}"
    );

    let mut plain = probierz();
    plain
        .args(["run", "mobile:ios:byk-auth", "--seed-resend"])
        .env("BYK_MAILBOX_BROKER", &not_executable);
    let (code, stderr) = stderr_of(plain);
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("is not an executable file"),
        "a non-executable broker must be refused as such: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn readiness_for_the_target_reports_the_mailbox_as_its_own_check() {
    let mut command = probierz();
    command
        .args(["check", "mobile:ios:byk-auth"])
        .env_remove("BYK_MAILBOX_BROKER");
    let output = command.output().expect("the product runs");
    let answer: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("check prints JSON");
    assert_eq!(answer["target"], "mobile:ios:byk-auth");
    let checks = answer["checks"].as_array().expect("checks array");
    let mailbox = checks
        .iter()
        .find(|row| row["name"] == "byk-ios-login mailbox reachable")
        .unwrap_or_else(|| panic!("no mailbox row in {answer}"));
    assert_eq!(
        mailbox["ok"], false,
        "no broker is provisioned on this host"
    );
    assert!(
        mailbox["hint"]
            .as_str()
            .expect("hint")
            .contains("BYK_MAILBOX_BROKER is required"),
        "the hint must name the variable: {mailbox}"
    );
    assert!(
        checks
            .iter()
            .any(|row| row["name"] == "app under test declared"),
        "readiness must include the application under test: {answer}"
    );
}
