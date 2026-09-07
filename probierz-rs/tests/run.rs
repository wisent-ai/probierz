#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;

#[test]
fn run_spawns_the_declared_npm_vector_with_the_run_environment() {
    let temporary = tempdir().expect("temporary directory");
    let harness = temporary.path().join("harness");
    let bin = temporary.path().join("bin");
    let capture = temporary.path().join("capture");
    fs::create_dir_all(harness.join("apps")).expect("apps directory");
    fs::create_dir_all(&bin).expect("bin directory");
    fs::create_dir_all(&capture).expect("capture directory");

    let npm = bin.join("npm");
    fs::write(
        &npm,
        r#"#!/bin/sh
printf '%s\n' "$@" > "$DRY_CAPTURE/argv"
pwd > "$DRY_CAPTURE/cwd"
printf '%s\n' "$FOO" "$PROBIERZ_APP_ID" "$PROBIERZ_TOOLKIT_ROOT" "$PROBIERZ_SPEC" "$PROBIERZ_RECORD" > "$DRY_CAPTURE/env"
printf '%s\n' "$PROBIERZ_ARTIFACTS" "$PROBIERZ_REPORT_PATH" > "$DRY_CAPTURE/paths"
printf '{"probierz":{"runId":"%s","captureErrors":[]},"total":1,"passed":1,"failed":0,"flaky":0,"skipped":0,"tests":[{"title":"dry invocation","passed":true,"status":"passed","duration":1,"media":[]}]}\n' "$PROBIERZ_RUN_ID" > "$PROBIERZ_REPORT_PATH"
"#,
    ).expect("fake npm");
    let mut permissions = fs::metadata(&npm).expect("npm metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&npm, permissions).expect("npm executable");

    let identity = temporary.path().join("source-identity.json");
    fs::write(
        &identity,
        format!(
            "{{\"schemaVersion\":1,\"appId\":\"probierz\",\"harness\":{{\"worktreeSha256\":\"{}\"}},\"app\":null}}\n",
            "0".repeat(64)
        ),
    ).expect("source identity");

    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .args([
            "--harness",
            harness.to_str().expect("UTF-8 harness"),
            "run",
            "web",
            "--force",
            "--no-analyze",
            "--record",
            "--spec",
            "focused.web.spec.ts",
            "--timeout",
            "5000",
            "FOO=bar",
        ])
        .env("PATH", path)
        .env("DRY_CAPTURE", &capture)
        .env("PROBIERZ_SOURCE_IDENTITY", &identity)
        .env("PROBIERZ_REPAIR_SUPPRESS", "1")
        .output()
        .expect("run command");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(capture.join("argv")).expect("argv"),
        "run\ntest:web\n"
    );
    let actual_cwd = fs::canonicalize(fs::read_to_string(capture.join("cwd")).expect("cwd").trim())
        .expect("actual cwd");
    assert_eq!(
        actual_cwd,
        fs::canonicalize(&harness).expect("canonical harness")
    );
    assert_eq!(
        fs::read_to_string(capture.join("env")).expect("env"),
        format!(
            "bar\nprobierz\n{}\nfocused.web.spec.ts\n1\n",
            harness.display()
        ),
    );
    let paths = fs::read_to_string(capture.join("paths")).expect("paths");
    let paths: Vec<&str> = paths.lines().collect();
    assert_eq!(paths.len(), 2);
    assert!(paths[0].starts_with(
        &harness
            .join("test-results/probierz/web")
            .to_string_lossy()
            .into_owned()
    ));
    assert_eq!(paths[1], format!("{}/report.json", paths[0]));

    let result: Value = serde_json::from_slice(&output.stdout).expect("run JSON");
    assert_eq!(
        result["command"],
        "npm run test:web (PROBIERZ_SPEC=focused.web.spec.ts)"
    );
    assert_eq!(result["passed"], true);
}

#[test]
fn byk_auth_run_uses_the_rust_bridge_instead_of_the_deleted_npm_runner() {
    let temporary = tempdir().expect("temporary directory");
    let harness = temporary.path().join("harness");
    let bin = temporary.path().join("bin");
    fs::create_dir_all(harness.join("apps")).expect("apps directory");
    fs::create_dir_all(&bin).expect("bin directory");
    let npm_called = temporary.path().join("npm-called");
    let npm = bin.join("npm");
    fs::write(
        &npm,
        format!("#!/bin/sh\ntouch '{}'\nexit 0\n", npm_called.display()),
    )
    .expect("fake npm");
    let mut permissions = fs::metadata(&npm).expect("npm metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&npm, permissions).expect("npm executable");
    let identity = temporary.path().join("source-identity.json");
    fs::write(
        &identity,
        format!(
            "{{\"schemaVersion\":1,\"appId\":\"probierz\",\"harness\":{{\"worktreeSha256\":\"{}\"}},\"app\":null}}\n",
            "0".repeat(64)
        ),
    ).expect("source identity");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .args([
            "--harness",
            harness.to_str().expect("UTF-8 harness"),
            "run",
            "mobile:ios:byk-auth",
            "--force",
            "--no-analyze",
        ])
        .env("PATH", path)
        .env("PROBIERZ_SOURCE_IDENTITY", &identity)
        .env("PROBIERZ_REPAIR_SUPPRESS", "1")
        .output()
        .expect("Byk run command");
    assert!(!output.status.success());
    assert!(
        !npm_called.exists(),
        "the removed JavaScript/npm runner was invoked"
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("run JSON");
    assert_eq!(
        result["stderrTail"],
        "byk auth runner: set exactly one of APP_IOS or BUNDLE_ID\n",
    );
    assert_eq!(result["exitCode"], 1);
}

#[test]
fn affected_classifies_package_and_cross_cutting_files() {
    let temporary = tempdir().expect("temporary directory");
    fs::create_dir_all(temporary.path().join("apps")).expect("apps directory");
    let output = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .args([
            "--harness",
            temporary.path().to_str().expect("UTF-8 harness"),
            "affected",
            "--files",
            "packages/mobile/specs/journey.ts",
            "docs/guide.md",
        ])
        .output()
        .expect("affected command");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("affected JSON");
    assert_eq!(
        result["targets"],
        serde_json::json!(["mobile:android", "mobile:ios", "mobile:ios:byk-auth"])
    );
    assert_eq!(result["crossCutting"], false);
    assert_eq!(
        result["files"][0]["affects"],
        serde_json::json!(["mobile:ios", "mobile:ios:byk-auth", "mobile:android"]),
    );
    assert_eq!(result["files"][1]["affects"], serde_json::json!([]));
}
