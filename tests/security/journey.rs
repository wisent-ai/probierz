//! Real Brama and owned-target journey; no simulated provider or HTTP server.
//! Run explicitly with --features security-live-tests --test security_journey.
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

const BINARY: &str = env!("CARGO_BIN_EXE_probierz");
fn required(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) => value,
        Err(error) => panic!("real security journey requires {name}: {error}"),
    }
}
fn run(harness: &Path, directory: &Path, label: &str, args: &[&str]) -> Output {
    let mut command = Command::new(BINARY);
    command.arg("--harness").arg(harness).args(args);
    if args.contains(&"--deterministic") {
        for name in [
            "STADO_MODEL_ROUTER_URL",
            "STADO_MODEL_ROUTER_TOKEN",
            "PROBIERZ_MODEL_AGENT_ID",
            "PROBIERZ_MODEL_AGENT_SECRET",
        ] {
            command.env_remove(name);
        }
    }
    let output = stado_wait::output(&mut command).expect("execute actual Probierz CLI");
    fs::write(directory.join(format!("{label}.stdout")), &output.stdout).expect("retain stdout");
    fs::write(directory.join(format!("{label}.stderr")), &output.stderr).expect("retain stderr");
    fs::write(
        directory.join(format!("{label}.command.json")),
        serde_json::to_vec_pretty(&json!({
            "binary": BINARY, "harness": harness, "arguments": args,
            "exitCode": output.status.code(), "success": output.status.success(),
            "modelEnvironmentRemoved": args.contains(&"--deterministic")
        }))
        .expect("serialize evidence"),
    )
    .expect("retain command evidence");
    output
}
/// Compare the observed finding classifications of policy and cookie checks
/// with the target contract's independently declared ones.
fn classified(row: &Value, check: &Value, expected: &Value, id: &str) {
    let (field, subject) = match check["kind"].as_str() {
        Some("policy") => ("failedAssertions", "header"),
        Some("cookies") => ("failedCookies", "cookie"),
        _ => return,
    };
    let findings = row[field]
        .as_array()
        .expect("every policy and cookie observation reports its findings");
    let actual: Vec<_> = findings
        .iter()
        .map(|finding| json!({subject: finding[subject], "reason": finding["reason"]}))
        .collect();
    assert_eq!(
        json!(actual),
        expected[field],
        "{field} classification differs from the target contract for {id}"
    );
    assert!(
        findings
            .iter()
            .all(|finding| finding.get("value").is_none()),
        "findings of {id} must not carry response values"
    );
}
struct OwnedManifest(PathBuf);
impl Drop for OwnedManifest {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("test manifest cleanup {}: {error}", self.0.display());
        }
    }
}

#[test]
fn real_owned_assessment_and_refusals_retain_evidence() {
    let harness = PathBuf::from(required("PROBIERZ_SECURITY_TEST_HARNESS"));
    let app = required("PROBIERZ_SECURITY_TEST_APP");
    let model = required("PROBIERZ_SECURITY_TEST_MODEL");
    let expected_path = PathBuf::from(required("PROBIERZ_SECURITY_TEST_EXPECTATIONS"));
    let expected: Value =
        serde_json::from_slice(&fs::read(&expected_path).expect("read real target expectations"))
            .expect("target expectations JSON");
    let expected = expected
        .as_object()
        .expect("expectations must map check IDs to pass and HTTP status");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string();
    let directory = harness.join(".build/security-results").join(&stamp);
    fs::create_dir_all(&directory).expect("create retained evidence directory");
    let report = directory.join("assessment.json");
    let report_arg = report.to_str().expect("UTF-8 report path");
    let args = [
        "evaluate", "security", "--app", &app, "--model", &model, "--out", report_arg,
    ];
    let output = run(&harness, &directory, "assessment", &args);
    let bytes = fs::read(&report).expect("actual evaluator must retain its report");
    let value: Value = serde_json::from_slice(&bytes).expect("report JSON");
    assert_eq!(
        value["complete"],
        true,
        "real flow blocked or failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let observed = value["observations"].as_array().expect("observations");
    let declared = value["scope"]["checks"]
        .as_array()
        .expect("declared checks");
    assert_eq!(
        observed.len(),
        declared.len(),
        "every authorized check must execute exactly once"
    );
    assert!(
        declared.iter().any(|check| check["kind"] == "policy"),
        "qualification must exercise explicit header policy"
    );
    assert!(
        declared.iter().any(|check| check["kind"] == "cors"),
        "qualification must exercise denied-origin CORS"
    );
    assert!(
        declared.iter().any(|check| check["kind"] == "cookies"),
        "qualification must exercise cookie attributes"
    );
    assert_eq!(
        expected.len(),
        declared.len(),
        "every declared check needs an independently declared expectation"
    );
    assert!(
        expected.values().any(|check| check["pass"] == true),
        "real fixture must include a passing check"
    );
    assert!(
        expected.values().any(|check| check["pass"] == false),
        "real fixture must include a security finding"
    );
    for check in declared {
        let matches: Vec<_> = observed
            .iter()
            .filter(|row| row["id"] == check["id"])
            .collect();
        let [row] = matches.as_slice() else {
            panic!("missing or duplicated check {}", check["id"]);
        };
        assert_eq!(row["path"], check["path"]);
        assert_eq!(row["method"], "GET");
        let id = check["id"].as_str().expect("declared check identity");
        let expected_check = expected.get(id).expect("check-specific target expectation");
        assert_eq!(
            row["pass"], expected_check["pass"],
            "unexpected security result for {id}"
        );
        assert_eq!(
            row["status"], expected_check["status"],
            "unexpected target status for {id}"
        );
        classified(row, check, expected_check, id);
    }
    assert_eq!(output.status.success(), value["pass"] == true);
    assert!(
        value["sourceIdentity"]["harness"]["worktreeSha256"].is_string(),
        "exact source identity must be retained"
    );
    let refused = run(&harness, &directory, "immutable-report", &args);
    assert!(
        !refused.status.success(),
        "existing evidence must be refused"
    );
    assert_eq!(fs::read(&report).expect("original evidence"), bytes);

    // Copy only the registered manifest into a new test-owned app, never edit operator data.
    let original = harness.join("apps").join(&app).join("probierz.yaml");
    let mut document: serde_yaml::Value =
        serde_yaml::from_slice(&fs::read(original).expect("registered manifest"))
            .expect("registered YAML");
    let test_id = format!("probierz-security-test-{stamp}");
    let test_app = OwnedManifest(harness.join("apps").join(&test_id));
    fs::create_dir(&test_app.0).expect("exclusive test application directory");
    document["appId"] = serde_yaml::Value::String(test_id.clone());
    document["security"]["owner"] = serde_yaml::Value::String(format!("not-the-owner-{stamp}"));
    fs::write(
        test_app.0.join("probierz.yaml"),
        serde_yaml::to_string(&document).expect("refusal declaration"),
    )
    .expect("test manifest");
    let rejected_report = directory.join("unauthorized.json");
    let rejected = run(
        &harness,
        &directory,
        "ownership-refusal",
        &[
            "evaluate",
            "security",
            "--app",
            &test_id,
            "--model",
            &model,
            "--out",
            rejected_report.to_str().expect("UTF-8 report"),
        ],
    );
    assert!(!rejected.status.success());
    assert!(
        !rejected_report.exists(),
        "ownership refusal must precede any run"
    );
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("security owner must match"),
        "{}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    eprintln!("real security journey evidence: {}", directory.display());
}

#[test]
fn real_deterministic_assessment_requires_no_model_credentials() {
    let harness = PathBuf::from(required("PROBIERZ_SECURITY_TEST_HARNESS"));
    let app = required("PROBIERZ_SECURITY_TEST_APP");
    let expected: Value = serde_json::from_slice(
        &fs::read(required("PROBIERZ_SECURITY_TEST_EXPECTATIONS"))
            .expect("read independent target expectations"),
    )
    .expect("target expectations JSON");
    let expected = expected.as_object().expect("expectations map");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string();
    let directory = harness
        .join(".build/security-results")
        .join(format!("deterministic-{stamp}"));
    fs::create_dir_all(&directory).expect("retained evidence directory");
    let report = directory.join("assessment.json");
    let report_arg = report.to_str().expect("UTF-8 report");
    let args = [
        "evaluate",
        "security",
        "--app",
        &app,
        "--deterministic",
        "--out",
        report_arg,
    ];
    let output = run(&harness, &directory, "assessment", &args);
    let bytes = fs::read(&report).expect("actual retained assessment");
    let value: Value = serde_json::from_slice(&bytes).expect("report JSON");
    assert_eq!(
        value["complete"],
        true,
        "deterministic assessment failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(value["mode"], "deterministic");
    assert!(
        value["modelRequested"].is_null() && value["plan"].is_null() && value["analysis"].is_null()
    );
    let observations = value["observations"].as_array().expect("observations");
    let declared = value["scope"]["checks"]
        .as_array()
        .expect("declared checks");
    assert_eq!(observations.len(), declared.len());
    assert_eq!(expected.len(), declared.len());
    for (observation, check) in observations.iter().zip(declared) {
        assert_eq!(
            observation["id"], check["id"],
            "deterministic execution preserves declaration order"
        );
        let id = check["id"].as_str().expect("check ID");
        let wanted = expected
            .get(id)
            .expect("independent expectation for every check");
        assert_eq!(
            observation["status"], wanted["status"],
            "unexpected HTTP status for {id}"
        );
        assert_eq!(
            observation["pass"], wanted["pass"],
            "unexpected security verdict for {id}"
        );
        classified(observation, check, wanted, id);
    }
    let journal_path = value["journal"]
        .as_str()
        .expect("retained operation journal path");
    let journal_bytes = fs::read(journal_path).expect("durable journal");
    let journal_text = std::str::from_utf8(&journal_bytes).expect("UTF-8 journal");
    assert!(
        journal_text.ends_with('\n'),
        "a successful journal must end with a complete record"
    );
    let events: Vec<Value> = journal_text
        .lines()
        .map(|line| serde_json::from_str(line).expect("journal event"))
        .collect();
    let retained: Vec<_> = events
        .iter()
        .filter(|event| event["state"] == "observed")
        .map(|event| &event["detail"])
        .collect();
    assert_eq!(
        retained,
        observations.iter().collect::<Vec<_>>(),
        "every completed observation must survive in the journal"
    );
    assert_eq!(
        events.last().expect("terminal journal event")["state"],
        "finished"
    );
    assert!(value["failedOperation"].is_null() && value["failedCheck"].is_null());
    let inspected = run(
        &harness,
        &directory,
        "inspect-journal",
        &["security-journal", journal_path],
    );
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let inspected_value: Value =
        serde_json::from_slice(&inspected.stdout).expect("journal inspection JSON");
    assert_eq!(inspected_value["partialRecord"], false);
    assert_eq!(inspected_value["events"], json!(events));
    assert!(
        inspected_value.get("pass").is_none(),
        "progress inspection must not declare a security pass"
    );
    let partial_path = directory.join("interrupted.events.jsonl");
    fs::write(
        &partial_path,
        journal_bytes
            .strip_suffix(b"\n")
            .expect("complete final newline"),
    )
    .expect("retain interrupted evidence");
    let partial_result = run(
        &harness,
        &directory,
        "inspect-partial",
        &[
            "security-journal",
            partial_path.to_str().expect("UTF-8 journal"),
        ],
    );
    assert!(partial_result.status.success());
    let partial_value: Value =
        serde_json::from_slice(&partial_result.stdout).expect("partial inspection JSON");
    assert_eq!(partial_value["partialRecord"], true);
    assert_ne!(
        partial_value["events"]
            .as_array()
            .expect("partial events")
            .last()
            .expect("retained events")["state"],
        "finished"
    );
    let trailing_path = directory.join("trailing.events.jsonl");
    let mut trailing = journal_bytes.clone();
    trailing.extend_from_slice(&journal_bytes);
    fs::write(&trailing_path, trailing).expect("retain invalid terminal sequence");
    let terminal_refusal = run(
        &harness,
        &directory,
        "inspect-terminal-refusal",
        &[
            "security-journal",
            trailing_path.to_str().expect("UTF-8 journal"),
        ],
    );
    assert!(
        !terminal_refusal.status.success(),
        "events after completion must be refused"
    );
    assert_eq!(output.status.success(), value["pass"] == true);
    let repeated = run(&harness, &directory, "immutable-report", &args);
    assert!(!repeated.status.success());
    assert_eq!(fs::read(&report).expect("original evidence"), bytes);
    assert_eq!(
        fs::read(journal_path).expect("unchanged journal"),
        journal_bytes
    );
    let conflict = directory.join("conflicting-mode.json");
    let refused = run(
        &harness,
        &directory,
        "conflicting-modes",
        &[
            "evaluate",
            "security",
            "--app",
            &app,
            "--deterministic",
            "--model",
            "undeclared-model",
            "--out",
            conflict.to_str().expect("UTF-8 path"),
        ],
    );
    assert!(!refused.status.success());
    assert!(
        !conflict.exists(),
        "conflicting modes must be refused before starting a run"
    );
    let incomplete_path = directory.join("invalid-model.json");
    let incomplete = run(
        &harness,
        &directory,
        "invalid-model",
        &[
            "evaluate",
            "security",
            "--app",
            &app,
            "--model",
            "",
            "--out",
            incomplete_path.to_str().expect("UTF-8 path"),
        ],
    );
    assert!(!incomplete.status.success());
    let failed: Value =
        serde_json::from_slice(&fs::read(&incomplete_path).expect("retained incomplete report"))
            .expect("incomplete JSON");
    assert_eq!(failed["complete"], false);
    assert_eq!(failed["pass"], false);
    assert_eq!(failed["failedOperation"], "model-configuration");
    assert_eq!(
        failed["observations"],
        json!([]),
        "invalid model configuration must precede target checks"
    );
    let failed_journal = fs::read_to_string(failed["journal"].as_str().expect("failure journal"))
        .expect("read failure journal");
    let terminal: Value =
        serde_json::from_str(failed_journal.lines().last().expect("terminal failure"))
            .expect("failure JSON");
    assert_eq!(terminal["state"], "failed");
    assert_eq!(terminal["operation"], "model-configuration");
    assert_eq!(terminal["detail"]["error"], failed["error"]);
    let occupied_report = directory.join("occupied.json");
    let occupied_journal = directory.join("occupied.json.events.jsonl");
    fs::copy(journal_path, &occupied_journal).expect("retain existing journal evidence");
    let occupied = run(
        &harness,
        &directory,
        "journal-collision",
        &[
            "evaluate",
            "security",
            "--app",
            &app,
            "--deterministic",
            "--out",
            occupied_report.to_str().expect("UTF-8 report"),
        ],
    );
    assert!(!occupied.status.success());
    let occupied_value: Value =
        serde_json::from_slice(&fs::read(&occupied_report).expect("collision report"))
            .expect("collision JSON");
    assert_eq!(occupied_value["failedOperation"], "journal-create");
    assert_eq!(occupied_value["observations"], json!([]));
    assert_eq!(
        fs::read(occupied_journal).expect("preserved journal"),
        journal_bytes
    );
    eprintln!(
        "real deterministic security evidence: {}",
        directory.display()
    );
}
