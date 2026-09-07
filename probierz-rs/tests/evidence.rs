use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn command(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root)
        .args(args)
        .output()
        .expect("run probierz")
}

fn empty_harness() -> TempDir {
    let root = TempDir::new().expect("temporary harness");
    fs::create_dir(root.path().join("apps")).expect("apps directory");
    root
}

#[test]
fn unknown_app_is_refused_with_the_exact_operator_sentence() {
    let root = empty_harness();
    let output = command(
        root.path(),
        &["retention", "unknown", "--at", "2026-09-06T00:00:00.000Z"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let detail = format!(
        "app manifest not found: {}",
        root.path().join("apps/unknown/probierz.yaml").display()
    );
    let expected = format!(
        "probierz-failure {{\"failure_point\":\"manifest.load\",\"error_code\":\"config\",\"service\":\"probierz\",\"retryable\":false,\"detail\":{}}}\nA declaration this command needs is missing or malformed; nothing ran.\n",
        serde_json::to_string(&detail).expect("JSON detail"),
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), expected);
}

#[test]
fn missing_run_is_refused_with_the_exact_operator_sentence() {
    let root = empty_harness();
    let output = command(
        root.path(),
        &["compare", "missing-left", "missing-right", "example"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "probierz-failure {\"failure_point\":\"evidence.run\",\"error_code\":\"invalid\",\"service\":\"probierz\",\"retryable\":false,\"detail\":\"run not found for example: missing-left\"}\nYour request was refused; nothing ran.\n",
    );
}

fn signed_receipt(tampered: bool) -> Value {
    json!({
        "schemaVersion": 3,
        "kind": "probierz-evidence-receipt",
        "appId": if tampered { "tampered" } else { "example" },
        "release": "v1",
        "issuedAt": "2026-09-06T00:00:00.000Z",
        "verdict": { "passed": true },
        "runs": [],
        "signing": {
            "algorithm": "Ed25519",
            "publicKeyFingerprintSha256": "174e59481418eb4eca03e70e90b26a7dda8e27d4986bb2df728aef59aa08bf14",
            "publicKeyPem": "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAtVKkh6d6PXTh1JyQyMFk634X8iHJdMcOWVu+/mvrTwI=\n-----END PUBLIC KEY-----\n",
            "payloadSha256": "0e7c2dad62c470d8d8becca0adff72d8cb39afc601851564f40ad371316b5bea",
            "signature": "GUxc2xNkXWjVc8/D6nlPWtEsz15pmpPeNEhyX8+K/ZRE8AD0eJB4T/W7SLOb1dDApYiE/qnrHwix06jB0UvOBg=="
        }
    })
}

#[test]
fn a_tampered_receipt_is_refused_without_reclassifying_it_as_an_io_failure() {
    let root = empty_harness();
    let receipt = root.path().join("tampered.json");
    let verify = || {
        Command::new(env!("CARGO_BIN_EXE_probierz"))
            .arg("--harness")
            .arg(root.path())
            .arg("verify-receipt")
            .arg(&receipt)
            .args([
                "--fingerprint",
                "174e59481418eb4eca03e70e90b26a7dda8e27d4986bb2df728aef59aa08bf14",
            ])
            .output()
            .expect("verify receipt")
    };
    fs::write(
        &receipt,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&signed_receipt(false)).expect("receipt JSON")
        ),
    )
    .expect("write valid receipt");
    let valid = verify();
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&valid.stdout).expect("valid verification JSON")["valid"],
        true
    );

    fs::write(
        &receipt,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&signed_receipt(true)).expect("receipt JSON")
        ),
    )
    .expect("tamper receipt");
    let output = verify();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stderr.is_empty(),
        "verification verdict is JSON, not a boundary failure sentence"
    );
    let answer: Value = serde_json::from_slice(&output.stdout).expect("verification JSON");
    assert_eq!(answer["valid"], false);
    assert_eq!(answer["signatureValid"], false);
    assert_eq!(answer["trusted"], true);
    assert_eq!(answer["appId"], "tampered");
}

fn harness_with_run() -> TempDir {
    let root = empty_harness();
    let app = root.path().join("apps/example");
    fs::create_dir_all(&app).expect("app directory");
    fs::write(
        app.join("probierz.yaml"),
        format!(
            "schemaVersion: 1\nappId: example\nowner: example maintainers\nrepositories:\n  - root: {}\n    mappings: []\nsurfaces:\n  tui:\n    spec: example.spec.mjs\n    journeys: [smoke]\njourneys:\n  smoke:\n    owner: example maintainers\n    timeoutMs: 1000\nartifacts:\n  retain:\n    adhocDays: 7\n",
            root.path().display(),
        ),
    )
    .expect("app manifest");
    let run = root
        .path()
        .join("test-results/example/tui/2026-09-06/run-one");
    fs::create_dir_all(&run).expect("run directory");
    let evidence = b"evidence\n";
    fs::write(run.join("proof.txt"), evidence).expect("evidence file");
    fs::write(
        run.join("run-manifest.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&json!({
                "schemaVersion": 2,
                "runId": "run-one",
                "appId": "example",
                "kind": "adhoc",
                "target": "tui",
                "spec": "example.spec.mjs",
                "status": "passed",
                "startedAt": "2026-09-06T00:00:00.000Z",
                "completedAt": "2026-09-06T00:00:01.000Z",
                "durationMs": 1000,
                "appManifest": { "journeys": ["smoke"] },
                "conditions": { "record": false },
                "artifacts": [{
                    "file": "proof.txt",
                    "sha256": hex::encode(Sha256::digest(evidence)),
                    "bytes": evidence.len(),
                }],
                "evidence": { "report": true, "analysis": true, "capturePresent": true },
            }))
            .expect("run JSON"),
        ),
    )
    .expect("run manifest");
    root
}

#[test]
fn protected_evidence_authenticates_and_restores_the_original_files() {
    let root = harness_with_run();
    let key = root.path().join("artifact.key");
    fs::write(&key, [7u8; 32]).expect("artifact key");
    let protected = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root.path())
        .args(["protect", "example", "run-one", "adhoc", "--key-file"])
        .arg(&key)
        .output()
        .expect("protect run");
    assert!(
        protected.status.success(),
        "{}",
        String::from_utf8_lossy(&protected.stderr)
    );
    let protected: Value = serde_json::from_slice(&protected.stdout).expect("protection JSON");
    assert_eq!(protected["secretScan"]["passed"], true);
    assert_eq!(protected["plaintextRemoved"], false);

    let destination = root.path().join("restored");
    let restored = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root.path())
        .arg("restore")
        .arg(protected["file"].as_str().expect("bundle path"))
        .arg(&destination)
        .arg("--key-file")
        .arg(&key)
        .output()
        .expect("restore bundle");
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    let restored: Value = serde_json::from_slice(&restored.stdout).expect("restore JSON");
    assert_eq!(restored["authenticated"], true);
    assert_eq!(
        fs::read(destination.join("proof.txt")).expect("restored evidence"),
        b"evidence\n"
    );

    let mut tampered =
        fs::read(protected["file"].as_str().expect("bundle path")).expect("bundle bytes");
    let tamper_at = tampered.len() - 17;
    tampered[tamper_at] ^= 1;
    let tampered_file = root.path().join("tampered.pev");
    fs::write(&tampered_file, tampered).expect("tampered bundle");
    let refused = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root.path())
        .arg("restore")
        .arg(&tampered_file)
        .arg(root.path().join("tampered-restore"))
        .arg("--key-file")
        .arg(&key)
        .output()
        .expect("restore tampered bundle");
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&refused.stderr),
        "probierz-failure {\"failure_point\":\"evidence.restore\",\"error_code\":\"invalid\",\"service\":\"probierz\",\"retryable\":false,\"detail\":\"encrypted evidence authentication failed: Unsupported state or unable to authenticate data\"}\nYour request was refused; nothing ran.\n",
    );
}
