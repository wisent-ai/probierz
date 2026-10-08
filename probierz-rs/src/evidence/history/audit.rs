use crate::evidence::*;
use serde_json::json;

pub(crate) fn files_below(root: &Path, reject_symlinks: bool) -> Result<Vec<PathBuf>, Failure> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() {
                if reject_symlinks {
                    return Err(Failure::invalid(
                        "evidence.protect",
                        format!(
                            "artifact source contains a symlink: {}",
                            entry.path().display()
                        ),
                    ));
                }
            } else if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

pub(crate) fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "auth",
        "cookie",
        "credential",
        "email",
        "key",
        "otp",
        "password",
        "pii",
        "secret",
        "session",
        "token",
    ]
    .iter()
    .any(|word| key.contains(word))
}

pub(crate) fn redact(value: &Value, key: &str) -> Value {
    if sensitive_key(key) {
        return json!("[REDACTED]");
    }
    match value {
        Value::Array(items) => Value::Array(items.iter().map(|item| redact(item, "")).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(name, item)| (name.clone(), redact(item, name)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

pub(crate) fn stable(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(stable).collect()),
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), stable(&object[key])))
                    .collect(),
            )
        }
        _ => value.clone(),
    }
}

pub(crate) fn random_uuid() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let text = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &text[0..8],
        &text[8..12],
        &text[12..16],
        &text[16..20],
        &text[20..32]
    )
}

pub(crate) fn audit_access(
    harness: &Path,
    action: &str,
    outcome: &str,
    app_id: Option<&str>,
    run_id: Option<&str>,
    resource: Option<&Path>,
    details: Value,
) -> Result<Value, Failure> {
    if action.is_empty() {
        return Err(Failure::invalid(
            "evidence.audit",
            "audit action is required",
        ));
    }
    let at = now_iso();
    let event_id = random_uuid();
    let actor = std::env::var("PROBIERZ_ACTOR")
        .or_else(|_| std::env::var("GITHUB_ACTOR"))
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string());
    let payload = json!({
        "schemaVersion": 1,
        "kind": "probierz-access-audit",
        "eventId": event_id,
        "at": at,
        "actor": actor,
        "action": action,
        "outcome": outcome,
        "appId": app_id,
        "runId": run_id,
        "resource": resource.map(|path| path.to_string_lossy().into_owned()),
        "context": {
            "ci": std::env::var("CI").is_ok_and(|value| !value.is_empty()),
            "workflow": std::env::var("GITHUB_WORKFLOW").ok(),
            "job": std::env::var("GITHUB_JOB").ok(),
        },
        "details": redact(&details, ""),
    });
    let checksum = sha256_bytes(serde_json::to_string(&stable(&payload))?.as_bytes());
    let mut record = payload.as_object().cloned().unwrap_or_default();
    record.insert("sha256".into(), json!(checksum));
    let directory = harness.join("test-results").join(".audit").join(&at[..10]);
    fs::create_dir_all(&directory)?;
    apply_mode(&directory, 0o700)?;
    let file = directory.join(format!("{}-{event_id}.json", at.replace([':', '.'], "-")));
    write_new_json(&file, &Value::Object(record), true)?;
    Ok(json!({ "eventId": event_id, "file": file.to_string_lossy(), "at": at, "sha256": checksum }))
}

pub(crate) fn audit_files(root: &Path) -> Result<Vec<PathBuf>, Failure> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut files = files_below(root, false)?;
    files.retain(|file| file.extension().and_then(|value| value.to_str()) == Some("json"));
    files.sort();
    Ok(files)
}
