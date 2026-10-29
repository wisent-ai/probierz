//! Exercise policy refusals against the real gate and genuine product evidence.
use std::path::{Path, PathBuf};
use std::process::Command;
use serde_json::json;
use serde_yaml::Value;
use sha2::{Digest, Sha256};

fn required(name: &str) -> String {
    let value = std::env::var(name).expect(name);
    assert!(!value.trim().is_empty(), "{name} is required for the real gate journey");
    value
}

fn copy_tree(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).expect("create isolated evidence directory");
    for entry in std::fs::read_dir(source).expect("read genuine evidence directory") {
        let entry = entry.expect("read genuine evidence entry");
        let kind = entry.file_type().expect("read evidence kind");
        assert!(!kind.is_symlink(), "evidence input must not contain symbolic links: {}", entry.path().display());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target.join(entry.file_name()));
        } else {
            assert!(kind.is_file(), "evidence input is not a regular file: {}", entry.path().display());
            std::fs::copy(entry.path(), target.join(entry.file_name())).expect("copy genuine evidence");
        }
    }
}

fn gate_configuration(file: &Path) -> Option<Vec<u8>> {
    match std::fs::read(file) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("cannot observe gate activation {}: {error}", file.display()),
    }
}

#[test]
fn gate_policy_refuses_missing_or_malformed_requirements_and_recovers() {
    let revision = required("PROBIERZ_TEST_SOURCE_REVISION");
    let source = PathBuf::from(required("PROBIERZ_TEST_GATE_HARNESS"));
    let release = PathBuf::from(required("PROBIERZ_TEST_RELEASE_EVIDENCE"));
    let app = required("PROBIERZ_TEST_GATE_APP");
    let mut app_components = Path::new(&app).components();
    assert!(matches!((app_components.next(), app_components.next()),
        (Some(std::path::Component::Normal(_)), None)), "gate app must name one app directory, not a path");
    let harness_sha = required("PROBIERZ_TEST_GATE_HARNESS_SHA");
    let source_sha = required("PROBIERZ_TEST_GATE_SOURCE_SHA");
    let runs = required("PROBIERZ_TEST_GATE_RUNS");
    let binary = Path::new(env!("CARGO_BIN_EXE_probierz"));
    let digest = hex::encode(Sha256::digest(std::fs::read(binary).expect("read tested binary")));
    assert_eq!(std::fs::read_to_string(release.join("SOURCE_REVISION")).expect("read release source").trim(), revision);
    let digests = std::fs::read_to_string(release.join("DIGESTS")).expect("read release digests");
    assert!(digests.lines().any(|line| line.split_whitespace().next() == Some(digest.as_str())), "tested binary is absent from release digests");
    let current = Command::new("git").args(["rev-parse", "HEAD"]).output().expect("read tested revision");
    assert!(current.status.success());
    assert_eq!(String::from_utf8(current.stdout).expect("revision text").trim(), revision);

    let results = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gate-policy");
    std::fs::create_dir_all(&results).expect("create report root");
    let report_root = tempfile::tempdir_in(&results).expect("create report directory").keep();
    let isolated = tempfile::tempdir_in(&report_root).expect("create isolated harness");
    let harness = isolated.path();
    copy_tree(&source.join("apps").join(&app), &harness.join("apps").join(&app));
    copy_tree(&source.join("test-results").join(&app), &harness.join("test-results").join(&app));
    let file = harness.join("apps").join(&app).join("probierz.yaml");
    let original = std::fs::read(&file).expect("read genuine manifest");
    let document: Value = serde_yaml::from_slice(&original).expect("decode genuine manifest");
    let key = Value::String("pullRequestPolicy".into());
    let genuine_policy = document.as_mapping().expect("manifest mapping").get(&key).expect("declared pull-request policy").clone();
    let harness_path = harness.to_string_lossy();
    let run = |name: &str, mode: &str| {
        let arguments = ["--harness", harness_path.as_ref(), "gate", "evaluate", app.as_str(),
            mode, harness_sha.as_str(), "--source-sha", source_sha.as_str(), "--runs", runs.as_str()];
        let output = Command::new(binary).args(arguments).output().expect("execute real gate");
        let record = json!({"sourceRevision": revision, "binarySha256": digest,
            "command": arguments, "exitCode": output.status.code(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr)});
        std::fs::write(report_root.join(format!("{name}.json")), serde_json::to_vec_pretty(&record).expect("encode command evidence")).expect("retain command evidence");
        output
    };
    let baseline = run("genuine-policy", "pull-request");
    assert!(baseline.status.success(), "genuine evidence must qualify before testing policy refusals: {}", String::from_utf8_lossy(&baseline.stderr));
    let before_config = gate_configuration(&file.with_file_name("gates.json"));
    let refuse_mode = |name: &str, field: &str, mode: &str, policy: Option<Value>, cause: &str| {
        let mut changed = document.clone();
        let map = changed.as_mapping_mut().expect("manifest mapping");
        let key = Value::String(field.into());
        match policy {
            Some(policy) => { map.insert(key, policy); }
            None => { map.remove(&key); }
        }
        std::fs::write(&file, serde_yaml::to_string(&changed).expect("encode policy variant")).expect("write isolated policy");
        let output = run(name, mode);
        let detail = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(!output.status.success(), "{name} unexpectedly passed: {detail}");
        assert!(detail.contains(cause) && detail.contains(file.to_string_lossy().as_ref()), "{name}: {detail}");
        assert_eq!(before_config, gate_configuration(&file.with_file_name("gates.json")), "policy evaluation changed gate activation");
    };
    let refuse = |name: &str, policy: Option<Value>, cause: &str| refuse_mode(name, "pullRequestPolicy", "pull-request", policy, cause);
    refuse_mode("missing-release-policy", "releasePolicy", "release", None, "releasePolicy is missing");
    refuse_mode("scalar-release-policy", "releasePolicy", "release", Some(Value::String("not a policy".into())), "releasePolicy must be a mapping");
    refuse("missing-policy", None, "pullRequestPolicy is missing");
    refuse("scalar-policy", Some(Value::String("not a policy".into())), "must be a mapping");
    refuse("null-policy", Some(Value::Null), "must be a mapping");
    let mut no_minimum = genuine_policy.clone();
    no_minimum.as_mapping_mut().expect("policy mapping").remove(&Value::String("minimumEvidence".into()));
    refuse_mode("missing-release-minimum", "releasePolicy", "release", Some(no_minimum.clone()), "minimumEvidence is missing");
    refuse("missing-minimum", Some(no_minimum), "minimumEvidence is missing");
    let malformed = |field: &str, value: Value, expected: &str, case: &str| {
        let mut policy = genuine_policy.clone();
        policy.as_mapping_mut().expect("policy mapping").insert(Value::String(field.into()), value);
        refuse(case, Some(policy), &format!("{field} {expected}"));
    };
    malformed("requireSecretScan", Value::String("true".into()), "must be a boolean", "text-scan-flag");
    malformed("requireProtectedArtifacts", Value::String("false".into()), "must be a boolean", "text-protection-flag");
    malformed("requireSecretScan", Value::Null, "must be a boolean", "null-scan-flag");
    malformed("requireProtectedArtifacts", Value::Null, "must be a boolean", "null-protection-flag");
    malformed("requiredTargets", Value::String("web".into()), "must be a list of non-empty text values", "scalar-targets");
    malformed("requiredTargets", Value::Sequence(vec![Value::Bool(true)]), "must be a list of non-empty text values", "nontext-target");
    malformed("requiredJourneys", Value::String("journey".into()), "must be a list of non-empty text values", "scalar-journeys");
    malformed("requiredJourneys", Value::Sequence(vec![Value::Bool(true)]), "must be a list of non-empty text values", "nontext-journey");
    malformed("requiredJourneys", Value::Null, "must be a list of non-empty text values", "null-journeys");
    malformed("minimumEvidence", Value::String("E+3".into()), "is not an E<n> evidence level", "invalid-minimum");
    malformed("requiredMatrixProfile", Value::String(String::new()), "must be non-empty text", "empty-matrix-profile");
    std::fs::write(&file, original).expect("restore original policy in isolated harness");
    let recovered = run("recovered-policy", "pull-request");
    assert!(recovered.status.success(), "{}", String::from_utf8_lossy(&recovered.stderr));
    std::fs::write(report_root.join("passed"), revision).expect("record completed real journey");
}
