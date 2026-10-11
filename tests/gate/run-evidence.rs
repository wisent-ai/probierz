use super::{all_runs, run_from_file};
use serde_json::Value;
use std::{env, fs, path::PathBuf, time::SystemTime};

#[test]
fn gate_evidence_preserves_read_failures_and_recovers() {
    let revision = env::var("PROBIERZ_TEST_SOURCE_REVISION").expect("candidate revision is required");
    let fixture = PathBuf::from(
        env::var("PROBIERZ_TEST_RUN_MANIFEST").expect("genuine run manifest is required"),
    );
    let original = fs::read(&fixture).expect("read genuine manifest");
    let document: Value = serde_json::from_slice(&original).expect("decode genuine manifest");
    let identity = document["runId"].as_str().expect("genuine run identity");
    assert!(!identity.is_empty());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/real-tests/gate-evidence");
    fs::create_dir_all(&root).unwrap();
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = root.join(format!("{}-{nonce}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let app = format!("evidence-{nonce}");
    let evidence = directory.join("test-results").join(&app);
    let run = evidence.join("retained-run");
    let manifest = run.join("run-manifest.json");
    assert!(all_runs(&directory, &app).unwrap().is_empty());
    assert!(run_from_file(&manifest).unwrap().is_none());
    fs::create_dir_all(&run).unwrap();
    fs::write(&manifest, &original).unwrap();
    let runs = all_runs(&directory, &app).unwrap();
    assert_eq!(
        runs.iter().map(|run| run.run_id.as_str()).collect::<Vec<_>>(),
        vec![identity]
    );
    let mut malformed = String::from_utf8(original.clone())
        .unwrap()
        .trim_end()
        .to_owned();
    assert_eq!(malformed.pop(), Some('}'));
    let cause = serde_json::from_slice::<Value>(malformed.as_bytes()).unwrap_err();
    fs::write(&manifest, malformed.as_bytes()).unwrap();
    let malformed_error = all_runs(&directory, &app).unwrap_err();
    assert!(malformed_error.contains(&manifest.display().to_string()));
    assert!(malformed_error.contains(&cause.to_string()));
    assert_eq!(fs::read(&manifest).unwrap(), malformed.as_bytes());
    fs::write(&manifest, &original).unwrap();

    let retained = directory.join("retained-evidence");
    fs::rename(&evidence, &retained).unwrap();
    fs::write(&evidence, &original).unwrap();
    let cause = fs::read_dir(&evidence).err().expect("file is not a directory");
    let traversal_error = all_runs(&directory, &app).unwrap_err();
    assert!(traversal_error.contains(&evidence.display().to_string()));
    assert!(traversal_error.contains(&cause.to_string()));
    assert_eq!(fs::read(&evidence).unwrap(), original);
    fs::remove_file(&evidence).unwrap();
    fs::rename(&retained, &evidence).unwrap();

    let saved = run.join("saved-manifest");
    fs::rename(&manifest, &saved).unwrap();
    fs::create_dir(&manifest).unwrap();
    let cause = fs::read_to_string(&manifest).unwrap_err();
    let read_error = run_from_file(&manifest).unwrap_err();
    assert!(read_error.contains(&manifest.display().to_string()));
    assert!(read_error.contains(&cause.to_string()));
    fs::remove_dir(&manifest).unwrap();
    fs::rename(&saved, &manifest).unwrap();
    let recovered = all_runs(&directory, &app).unwrap();
    assert_eq!(
        recovered.iter().map(|run| run.run_id.as_str()).collect::<Vec<_>>(),
        vec![identity]
    );
    assert_eq!(fs::read(&manifest).unwrap(), original);
    assert_eq!(fs::read(&fixture).unwrap(), original);
    let report = serde_json::json!({
        "revision": revision,
        "fixture": fixture,
        "runId": identity,
        "diagnostics": [malformed_error, traversal_error, read_error],
        "recoveredManifest": manifest,
        "verdict": "passed"
    });
    let report_path = directory.join("report.json");
    fs::write(&report_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("{}", report_path.display());
}
