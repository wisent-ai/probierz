use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::TempDir;

fn harness_with_app(root: &Path, repository: &Path) {
    let app = root.join("apps/example");
    fs::create_dir_all(&app).expect("create app directory");
    fs::create_dir_all(root.join("test-results")).expect("create results directory");
    fs::write(
        app.join("probierz.yaml"),
        format!(
            "schemaVersion: 1\nappId: example\nowner: example maintainers\nrepositories:\n  - root: {}\n    mappings: []\nsurfaces:\n  tui:\n    spec: example.spec.mjs\n    journeys: [smoke]\njourneys:\n  smoke:\n    owner: example maintainers\n    timeoutMs: 1000\npullRequestPolicy:\n  minimumEvidence: E2\n",
            repository.display()
        ),
    )
    .expect("write app manifest");
}

fn passing_run(root: &Path) {
    let directory = root.join("test-results/example/tui/2026-09-06/run-green");
    fs::create_dir_all(&directory).expect("create run directory");
    fs::write(
        directory.join("run-manifest.json"),
        r#"{
  "schemaVersion": 2,
  "runId": "run-green",
  "appId": "example",
  "kind": "pull-request",
  "target": "tui",
  "spec": "example.spec.mjs",
  "status": "passed",
  "startedAt": "2026-09-06T12:00:00.000Z",
  "completedAt": "2026-09-06T12:00:01.000Z",
  "harness": {
    "sha256": "harness-sha",
    "gitSha": "1111111111111111111111111111111111111111",
    "worktreeSha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  },
  "source": {
    "sha256": "source-sha",
    "repositories": [{
      "index": 0,
      "gitSha": "2222222222222222222222222222222222222222",
      "worktreeSha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    }]
  },
  "build": { "sha256": "build-sha" },
  "appManifest": { "journeys": ["smoke"] },
  "conditions": { "record": false },
  "artifacts": [],
  "evidence": { "report": true, "analysis": true, "capturePresent": false }
}
"#,
    )
    .expect("write passing run");
}

fn install(binary: &str, harness: &Path, repo: &Path) -> serde_json::Value {
    let output = Command::new(binary)
        .arg("--harness")
        .arg(harness)
        .args(["gate-install", "example", "--repo"])
        .arg(repo)
        .output()
        .expect("run gate-install");
    assert!(
        output.status.success(),
        "gate-install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("gate-install JSON")
}

#[test]
fn installed_hook_runs_the_rust_gate_and_chains_an_existing_hook() {
    let temporary = TempDir::new().expect("temporary directory");
    let repo = temporary.path().join("repo");
    let hooks = repo.join(".git/hooks");
    fs::create_dir_all(&hooks).expect("create hooks directory");
    harness_with_app(temporary.path(), &repo);

    let original = hooks.join("pre-push");
    fs::write(&original, "#!/bin/sh\necho existing-hook\n").expect("write original hook");
    fs::set_permissions(&original, fs::Permissions::from_mode(0o755))
        .expect("make original hook executable");

    let binary = env!("CARGO_BIN_EXE_probierz");
    let result = install(binary, temporary.path(), &repo);
    assert_eq!(result["chained"], true);
    assert!(hooks.join("pre-push.before-probierz-gate").exists());

    let installed = fs::read_to_string(hooks.join("pre-push")).expect("read installed hook");
    assert!(
        installed.contains(binary),
        "hook must invoke the Rust binary"
    );
    assert!(installed.contains("gate-prepush --hook --app 'example'"));
    assert!(!installed.contains("prepush-gate.mjs"));
    assert_ne!(
        fs::metadata(hooks.join("pre-push"))
            .expect("hook metadata")
            .mode()
            & 0o111,
        0
    );

    let mut child = Command::new(hooks.join("pre-push"))
        .env("PROBIERZ_GATE_NO_CI", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run installed hook");
    child.stdin.as_mut().expect("hook stdin").write_all(
        b"refs/heads/topic 1111111111111111111111111111111111111111 refs/heads/topic 0000000000000000000000000000000000000000\n",
    ).expect("write pushed refs");
    let output = child.wait_with_output().expect("wait for installed hook");
    assert!(
        output.status.success(),
        "installed hook failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "existing-hook\nprepush-gate: push does not target main; allowed\n"
    );
}

#[test]
fn reinstall_replaces_the_managed_javascript_hook_instead_of_chaining_it() {
    let temporary = TempDir::new().expect("temporary directory");
    let repo = temporary.path().join("repo");
    let hooks = repo.join(".git/hooks");
    fs::create_dir_all(&hooks).expect("create hooks directory");
    harness_with_app(temporary.path(), &repo);
    fs::write(
        hooks.join("pre-push"),
        "#!/bin/sh\n# managed-by: probierz-prepush-gate\nexec node /old/probierz/agent/prepush-gate.mjs --hook --app example --ci\n",
    )
    .expect("write legacy managed hook");

    let binary = env!("CARGO_BIN_EXE_probierz");
    let result = install(binary, temporary.path(), &repo);
    assert_eq!(result["chained"], false);
    assert!(!hooks.join("pre-push.before-probierz-gate").exists());
    let installed = fs::read_to_string(hooks.join("pre-push")).expect("read replacement hook");
    assert!(installed.contains(binary));
    assert!(!installed.contains("prepush-gate.mjs"));
}

#[test]
fn prepush_refusal_returns_exit_one_and_its_exact_reason() {
    let temporary = TempDir::new().expect("temporary directory");
    let declared_repo = temporary.path().join("declared-repo");
    harness_with_app(temporary.path(), &declared_repo);
    let unmatched = temporary.path().join("unmatched-repo");
    let output = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(temporary.path())
        .args(["gate-prepush", "--repo"])
        .arg(&unmatched)
        .output()
        .expect("run blocked prepush gate");
    assert_eq!(output.status.code(), Some(1));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).expect("prepush JSON");
    assert_eq!(
        result["reason"],
        format!("no probierz app manifest matches {}", unmatched.display())
    );
}

#[test]
fn green_activation_persists_and_enforcement_uses_the_required_gate() {
    let temporary = TempDir::new().expect("temporary directory");
    let repo = temporary.path().join("repo");
    harness_with_app(temporary.path(), &repo);
    passing_run(temporary.path());
    let binary = env!("CARGO_BIN_EXE_probierz");
    let common = [
        "example",
        "pull-request",
        "harness-sha",
        "--source-sha",
        "source-sha",
        "--runs",
        "run-green",
    ];

    let activation = Command::new(binary)
        .arg("--harness")
        .arg(temporary.path())
        .arg("gate-activate")
        .args(common)
        .output()
        .expect("activate gate");
    assert!(
        activation.status.success(),
        "activation failed: {}",
        String::from_utf8_lossy(&activation.stderr)
    );
    let activation: serde_json::Value =
        serde_json::from_slice(&activation.stdout).expect("activation JSON");
    assert_eq!(
        activation["config"]["modes"]["pull-request"]["enforcement"],
        "required"
    );
    let gates: serde_json::Value = serde_json::from_slice(
        &fs::read(temporary.path().join("apps/example/gates.json")).expect("read persisted gate"),
    )
    .expect("persisted gate JSON");
    assert_eq!(gates, activation["config"]);

    let enforcement = Command::new(binary)
        .arg("--harness")
        .arg(temporary.path())
        .arg("gate-enforce")
        .args(common)
        .output()
        .expect("enforce gate");
    assert!(
        enforcement.status.success(),
        "enforcement failed: {}",
        String::from_utf8_lossy(&enforcement.stderr)
    );
    let enforcement: serde_json::Value =
        serde_json::from_slice(&enforcement.stdout).expect("enforcement JSON");
    assert_eq!(enforcement["verdict"]["passed"], true);
    assert_eq!(
        enforcement["status"]["modes"]["pull-request"]["enforcement"],
        "required"
    );
}
