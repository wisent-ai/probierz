use super::update_json;
use serde_json::Value;
use std::{env, fs, path::PathBuf, time::SystemTime};

#[test]
fn manifest_updates_preserve_failed_inputs_and_recover() {
    let revision =
        env::var("PROBIERZ_TEST_SOURCE_REVISION").expect("candidate revision is required");
    let fixture = PathBuf::from(
        env::var("PROBIERZ_TEST_RUN_MANIFEST").expect("real run manifest is required"),
    );
    let original = fs::read(&fixture).expect("read genuine run manifest");
    let document: Value = serde_json::from_slice(&original).expect("decode genuine run manifest");
    let object = document
        .as_object()
        .expect("real manifest must be an object");
    let identity = document
        .get("runId")
        .and_then(Value::as_str)
        .expect("real run identity");
    assert!(!identity.is_empty(), "real run identity must not be empty");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/real-tests/manifest-update");
    fs::create_dir_all(&root).expect("create evidence root");
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = root.join(format!("{}-{nonce}", std::process::id()));
    fs::create_dir(&directory).expect("create isolated evidence directory");
    let path = directory.join("run-manifest.json");
    let mut report =
        serde_json::json!({"revision": revision, "fixture": fixture, "diagnostics": []});
    let mut diagnostics = Vec::new();

    update_json(&path, &document).expect("initialize a missing manifest");
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(),
        document
    );
    let (key, value) = object.iter().next().expect("manifest fields");
    let mut partial = object.clone();
    partial.remove(key);
    fs::write(&path, serde_json::to_vec(&partial).unwrap()).unwrap();
    let patch = Value::Object([(key.clone(), value.clone())].into_iter().collect());
    update_json(&path, &patch).expect("merge the missing actual manifest field");
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(),
        document
    );

    let mut malformed = String::from_utf8(original.clone())
        .expect("manifest UTF-8")
        .trim_end()
        .to_owned();
    assert_eq!(malformed.pop(), Some('}'));
    let parse_error = serde_json::from_slice::<Value>(malformed.as_bytes()).unwrap_err();
    fs::write(&path, malformed.as_bytes()).unwrap();
    let refused = update_json(&path, &patch).unwrap_err().to_string();
    assert!(refused.contains(&path.display().to_string()), "{refused}");
    assert!(refused.contains(&parse_error.to_string()), "{refused}");
    assert_eq!(fs::read(&path).unwrap(), malformed.as_bytes());
    diagnostics.push(refused);

    let scalar = serde_json::to_vec(identity).unwrap();
    fs::write(&path, &scalar).unwrap();
    let refused = update_json(&path, &patch).unwrap_err().to_string();
    assert!(
        refused.contains(&path.display().to_string()) && refused.contains("JSON object"),
        "{refused}"
    );
    assert_eq!(fs::read(&path).unwrap(), scalar);
    diagnostics.push(refused);

    fs::write(&path, &original).unwrap();
    let refused = update_json(&path, &Value::String(identity.to_owned()))
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains(&path.display().to_string()) && refused.contains("patch"),
        "{refused}"
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    diagnostics.push(refused);

    let retained = directory.join("retained-manifest.json");
    fs::rename(&path, &retained).unwrap();
    fs::create_dir(&path).unwrap();
    let read_error = fs::read(&path).unwrap_err();
    let refused = update_json(&path, &patch).unwrap_err().to_string();
    assert!(refused.contains(&path.display().to_string()), "{refused}");
    assert!(refused.contains(&read_error.to_string()), "{refused}");
    assert!(path.is_dir());
    assert_eq!(fs::read(&retained).unwrap(), original);
    diagnostics.push(refused);
    fs::remove_dir(&path).unwrap();
    fs::rename(&retained, &path).unwrap();
    update_json(&path, &patch).expect("recover after refused reads");
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(),
        document
    );
    assert_eq!(
        fs::read(&fixture).unwrap(),
        original,
        "genuine source manifest must stay unchanged"
    );
    report["diagnostics"] = serde_json::to_value(diagnostics).unwrap();
    fs::write(
        directory.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    eprintln!("manifest update evidence: {}", directory.display());
}
