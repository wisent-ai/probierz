//! Durable evidence: history, signing, publication, protection, and audit.
//!
//! Evidence is a security boundary. Paths are kept beneath their declared
//! roots, signed payloads are canonicalized before Ed25519 operations, and an
//! encrypted bundle is authenticated before any member is restored.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use aes::cipher::{BlockEncrypt, KeyInit, KeyIvInit, StreamCipher};
use aes::Aes256;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::{DateTime, SecondsFormat, Utc};
use ctr::Ctr32BE;
use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
use ed25519_dalek::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePublicKey};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use ghash::universal_hash::UniversalHash;
use ghash::GHash;
use rand_core::{OsRng, RngCore};
use regex::Regex;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use url::Url;

use crate::failure::{now_iso, print_json, Answer, Failure};
use crate::manifest;

const MAGIC: &[u8] = b"PROBIERZ-EVIDENCE-1\n";
const TAG_BYTES: usize = 16;

fn sha256_bytes(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

fn sha256_file(file: &Path) -> Result<String, Failure> {
    let mut input = File::open(file)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            let body = items.iter().map(canonical).collect::<Vec<_>>().join(",");
            format!("[{body}]")
        }
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            let body = keys
                .into_iter()
                .map(|key| {
                    let encoded = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string());
                    format!("{encoded}:{}", canonical(&object[key]))
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
        _ => serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()),
    }
}

fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

fn yaml_json(value: &serde_yaml::Value) -> Result<Value, Failure> {
    Ok(serde_json::to_value(value)
        .map_err(|error| Failure::config("evidence.manifest", error.to_string()))?)
}

fn json_file(path: &Path) -> Result<Value, Failure> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn try_json_file(path: &Path) -> Option<Value> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn manifests_below(root: &Path) -> Result<Vec<PathBuf>, Failure> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut pending = vec![root.to_path_buf()];
    let mut found = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() && entry.file_name() == "run-manifest.json" {
                found.push(entry.path());
            }
        }
    }
    Ok(found)
}

fn normalized_status(manifest: &Value) -> String {
    match manifest.get("status").and_then(Value::as_str) {
        Some("passed" | "executed") => "passed",
        Some("blocked") => "blocked",
        Some("canceled") => "canceled",
        Some("failed") => "failed",
        _ if manifest
            .get("completedAt")
            .is_some_and(|value| !value.is_null()) =>
        {
            "failed"
        }
        _ => "incomplete",
    }
    .to_string()
}

fn tests_from(run_directory: &Path, manifest_value: &Value) -> Vec<Value> {
    let analysis_path = manifest_value
        .get("analysisPath")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| run_directory.join("analysis.json"));
    let report_path = manifest_value
        .pointer("/paths/reportPath")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| run_directory.join("report.json"));
    let analysis = try_json_file(&analysis_path);
    let report = try_json_file(&report_path);
    let source = analysis
        .as_ref()
        .and_then(|value| value.get("tests"))
        .and_then(Value::as_array)
        .or_else(|| {
            report
                .as_ref()
                .and_then(|value| value.get("tests"))
                .and_then(Value::as_array)
        });
    let mut order = Vec::<String>::new();
    let mut by_title = HashMap::<String, Value>::new();
    for test in source.into_iter().flatten() {
        let title = test
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !by_title.contains_key(&title) {
            order.push(title.clone());
        }
        let status = test
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                if test.get("passed").and_then(Value::as_bool).unwrap_or(false) {
                    "passed"
                } else {
                    "failed"
                }
                .to_string()
            });
        let duration = test
            .get("durationMs")
            .or_else(|| test.get("duration"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        by_title.insert(
            title.clone(),
            json!({ "title": title, "status": status, "durationMs": js_number(duration) }),
        );
    }
    order
        .into_iter()
        .filter_map(|title| by_title.remove(&title))
        .collect()
}

fn js_number(value: f64) -> Value {
    if !value.is_finite() {
        Value::Null
    } else if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        json!(value as i64)
    } else {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

fn run_record(manifest_path: &Path) -> Option<Value> {
    let source = try_json_file(manifest_path)?;
    let directory = manifest_path.parent()?;
    let analysis_path = source
        .get("analysisPath")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("analysis.json"));
    let report_path = source
        .pointer("/paths/reportPath")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("report.json"));
    let analysis = try_json_file(&analysis_path);
    let report = try_json_file(&report_path);
    let failures = analysis
        .as_ref()
        .and_then(|value| value.get("failures"))
        .and_then(Value::as_array)
        .or_else(|| {
            report
                .as_ref()
                .and_then(|value| value.get("failures"))
                .and_then(Value::as_array)
        });
    let failure_text = failures
        .into_iter()
        .flatten()
        .map(|failure| {
            failure
                .get("error")
                .or_else(|| failure.get("message"))
                .and_then(Value::as_str)
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let infrastructure = [
        "executable doesn't exist",
        "toolchain",
        "connection refused",
        "econnrefused",
    ]
    .iter()
    .any(|needle| failure_text.contains(needle))
        || (failure_text.contains("driver") && failure_text.contains("not installed"));
    let status = normalized_status(&source);
    let mut record = Map::new();
    record.insert(
        "runId".into(),
        source.get("runId").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "appId".into(),
        source.get("appId").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "kind".into(),
        source
            .get("kind")
            .cloned()
            .unwrap_or_else(|| json!("adhoc")),
    );
    record.insert(
        "target".into(),
        source.get("target").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "spec".into(),
        source.get("spec").cloned().unwrap_or(Value::Null),
    );
    record.insert("status".into(), json!(status));
    record.insert(
        "startedAt".into(),
        source.get("startedAt").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "completedAt".into(),
        source.get("completedAt").cloned().unwrap_or(Value::Null),
    );
    let duration = source
        .get("durationMs")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    record.insert("durationMs".into(), js_number(duration));
    for key in ["harness", "source", "build"] {
        record.insert(key.into(), source.get(key).cloned().unwrap_or(Value::Null));
    }
    record.insert(
        "journeys".into(),
        source
            .pointer("/appManifest/journeys")
            .cloned()
            .unwrap_or_else(|| json!([])),
    );
    record.insert(
        "failureClass".into(),
        if record.get("status").and_then(Value::as_str) == Some("failed") {
            json!(if infrastructure {
                "infrastructure"
            } else {
                "product"
            })
        } else {
            Value::Null
        },
    );
    record.insert(
        "device".into(),
        source.get("device").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "conditions".into(),
        source
            .get("conditions")
            .cloned()
            .unwrap_or_else(|| json!({})),
    );
    record.insert(
        "evidence".into(),
        source.get("evidence").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "artifacts".into(),
        source
            .get("artifacts")
            .cloned()
            .unwrap_or_else(|| json!([])),
    );
    record.insert(
        "protection".into(),
        source.get("protection").cloned().unwrap_or(Value::Null),
    );
    record.insert(
        "manifestPath".into(),
        json!(manifest_path.to_string_lossy()),
    );
    record.insert(
        "analysisPath".into(),
        source.get("analysisPath").cloned().unwrap_or(Value::Null),
    );
    record.insert("tests".into(), Value::Array(tests_from(directory, &source)));
    Some(Value::Object(record))
}

fn get_run(harness: &Path, app_id: &str, run_id: &str) -> Result<Value, Failure> {
    for file in manifests_below(&harness.join("test-results").join(app_id))? {
        if let Some(run) = run_record(&file) {
            if run.get("runId").and_then(Value::as_str) == Some(run_id) {
                return Ok(run);
            }
        }
    }
    Err(Failure::invalid(
        "evidence.run",
        format!("run not found for {app_id}: {run_id}"),
    ))
}

pub fn compare(
    harness: &Path,
    left_id: Option<&str>,
    right_id: Option<&str>,
    app_id: Option<&str>,
) -> Answer {
    let left_id = left_id.ok_or_else(|| {
        Failure::invalid("evidence.compare", "compare needs left and right run IDs")
    })?;
    let right_id = right_id.ok_or_else(|| {
        Failure::invalid("evidence.compare", "compare needs left and right run IDs")
    })?;
    let app_id = app_id.unwrap_or("probierz");
    let left = get_run(harness, app_id, left_id)?;
    let right = get_run(harness, app_id, right_id)?;
    let tests = compare_named(&left, &right, "tests", "title", true);
    let artifacts = compare_named(&left, &right, "artifacts", "file", false);
    let left_duration = left
        .get("durationMs")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let right_duration = right
        .get("durationMs")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let ratio = if left_duration > 0.0 {
        Some(right_duration / left_duration)
    } else {
        None
    };
    let newly_failing = tests
        .1
        .iter()
        .filter(|change| {
            change.pointer("/after/status").and_then(Value::as_str) == Some("failed")
                && change.pointer("/before/status").and_then(Value::as_str) != Some("failed")
        })
        .filter_map(|change| change.get("title").cloned())
        .collect::<Vec<_>>();
    let side = |run: &Value| {
        json!({
            "runId": run.get("runId").cloned().unwrap_or(Value::Null),
            "status": run.get("status").cloned().unwrap_or(Value::Null),
            "harness": run.get("harness").cloned().unwrap_or(Value::Null),
            "source": run.get("source").cloned().unwrap_or(Value::Null),
            "build": run.get("build").cloned().unwrap_or(Value::Null),
            "durationMs": run.get("durationMs").cloned().unwrap_or_else(|| json!(0)),
            "evidence": run.get("evidence").cloned().unwrap_or(Value::Null),
        })
    };
    print_json(&json!({
        "schemaVersion": 2,
        "appId": app_id,
        "left": side(&left),
        "right": side(&right),
        "verdict": {
            "statusChanged": left.get("status") != right.get("status"),
            "regression": right.get("status").and_then(Value::as_str) == Some("failed") && left.get("status").and_then(Value::as_str) == Some("passed"),
            "newlyFailing": newly_failing,
            "durationRegression": ratio.is_some_and(|value| left_duration >= 500.0 && value >= 1.2),
        },
        "duration": { "deltaMs": js_number(right_duration - left_duration), "ratio": ratio.map(js_number).unwrap_or(Value::Null) },
        "tests": { "changed": tests.0, "changes": tests.1 },
        "artifacts": { "changed": artifacts.0, "changes": artifacts.1 },
    }))
}

fn compare_named(
    left: &Value,
    right: &Value,
    collection: &str,
    name: &str,
    test: bool,
) -> (usize, Vec<Value>) {
    let indexed = |run: &Value| -> HashMap<String, Value> {
        run.get(collection)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                item.get(name)
                    .and_then(Value::as_str)
                    .map(|key| (key.to_string(), item.clone()))
            })
            .collect()
    };
    let before = indexed(left);
    let after = indexed(right);
    let mut names: BTreeSet<String> = before.keys().cloned().collect();
    names.extend(after.keys().cloned());
    let mut changes = Vec::new();
    for entry_name in names {
        match (before.get(&entry_name), after.get(&entry_name)) {
            (None, Some(value)) => changes.push(
                json!({ name: entry_name, "change": "added", "before": null, "after": value }),
            ),
            (Some(value), None) => changes.push(
                json!({ name: entry_name, "change": "removed", "before": value, "after": null }),
            ),
            (Some(old), Some(new)) if test => {
                let old_status = old.get("status");
                let new_status = new.get("status");
                let old_duration = old.get("durationMs").and_then(Value::as_f64).unwrap_or(0.0);
                let new_duration = new.get("durationMs").and_then(Value::as_f64).unwrap_or(0.0);
                if old_status != new_status || old_duration != new_duration {
                    changes.push(json!({
                        name: entry_name,
                        "change": if old_status == new_status { "duration" } else { "status" },
                        "before": old,
                        "after": new,
                        "durationDeltaMs": js_number(new_duration - old_duration),
                    }));
                }
            }
            (Some(old), Some(new)) => {
                if old.get("sha256") != new.get("sha256") || old.get("bytes") != new.get("bytes") {
                    changes.push(json!({ name: entry_name, "change": "content", "before": old, "after": new }));
                }
            }
            _ => {}
        }
    }
    (changes.len(), changes)
}

pub fn last_green(
    harness: &Path,
    app_id: Option<&str>,
    target: Option<&str>,
    journey: Option<&str>,
) -> Answer {
    let app_id = app_id.unwrap_or("probierz");
    let root = match target {
        Some(value) => harness
            .join("test-results")
            .join(app_id)
            .join(value.replace(':', "-")),
        None => harness.join("test-results").join(app_id),
    };
    let mut runs = manifests_below(&root)?
        .into_iter()
        .filter_map(|file| run_record(&file))
        .filter(|run| {
            target.is_none_or(|wanted| run.get("target").and_then(Value::as_str) == Some(wanted))
        })
        .collect::<Vec<_>>();
    runs.sort_by(|left, right| {
        right
            .get("startedAt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(
                left.get("startedAt")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
    });
    let run = runs.into_iter().find(|candidate| {
        candidate.get("status").and_then(Value::as_str) == Some("passed")
            && journey.is_none_or(|wanted| {
                candidate
                    .get("journeys")
                    .and_then(Value::as_array)
                    .is_some_and(|names| names.iter().any(|name| name.as_str() == Some(wanted)))
            })
    });
    print_json(
        &json!({ "schemaVersion": 2, "appId": app_id, "target": target, "journey": journey, "run": run }),
    )
}

fn files_below(root: &Path, reject_symlinks: bool) -> Result<Vec<PathBuf>, Failure> {
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

fn sensitive_key(key: &str) -> bool {
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

fn redact(value: &Value, key: &str) -> Value {
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

fn stable(value: &Value) -> Value {
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

fn random_uuid() -> String {
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

fn audit_access(
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

fn audit_files(root: &Path) -> Result<Vec<PathBuf>, Failure> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut files = files_below(root, false)?;
    files.retain(|file| file.extension().and_then(|value| value.to_str()) == Some("json"));
    files.sort();
    Ok(files)
}

pub fn audit(
    harness: &Path,
    app_id: Option<&str>,
    run_id: Option<&str>,
    action: Option<&str>,
    limit: &str,
) -> Answer {
    let parsed = limit
        .parse::<f64>()
        .map_err(|_| Failure::invalid("evidence.audit", "--limit needs a positive number"))?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return Err(Failure::invalid(
            "evidence.audit",
            "--limit needs a positive number",
        ));
    }
    let mut records = Vec::new();
    for file in audit_files(&harness.join("test-results").join(".audit"))? {
        match json_file(&file) {
            Ok(record) => {
                if app_id.is_some_and(|wanted| {
                    record.get("appId").and_then(Value::as_str) != Some(wanted)
                }) || run_id.is_some_and(|wanted| {
                    record.get("runId").and_then(Value::as_str) != Some(wanted)
                }) || action.is_some_and(|wanted| {
                    record.get("action").and_then(Value::as_str) != Some(wanted)
                }) {
                    continue;
                }
                let mut payload = record.as_object().cloned().unwrap_or_default();
                let expected = payload
                    .remove("sha256")
                    .and_then(|value| value.as_str().map(str::to_string));
                let valid = expected.as_deref()
                    == Some(&sha256_bytes(
                        serde_json::to_string(&stable(&Value::Object(payload)))?.as_bytes(),
                    ));
                let mut output = record.as_object().cloned().unwrap_or_default();
                output.insert("valid".into(), json!(valid));
                output.insert("file".into(), json!(file.to_string_lossy()));
                records.push(Value::Object(output));
            }
            Err(error) => records.push(
                json!({ "valid": false, "file": file.to_string_lossy(), "error": error.detail }),
            ),
        }
    }
    records.sort_by(|left, right| {
        right
            .get("at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(left.get("at").and_then(Value::as_str).unwrap_or_default())
    });
    let total = records.len();
    records.truncate((parsed as usize).max(1));
    let valid = records
        .iter()
        .filter(|record| record.get("valid").and_then(Value::as_bool) == Some(true))
        .count();
    print_json(&json!({
        "schemaVersion": 1,
        "filters": { "appId": app_id, "runId": run_id, "action": action },
        "total": total,
        "returned": records.len(),
        "valid": valid,
        "invalid": records.len() - valid,
        "records": records,
    }))
}

fn acceptable_secret(value: &str) -> bool {
    let environment_reference = ["env.", "source.", "process.env."].iter().any(|prefix| {
        let Some(name) = value.strip_prefix(prefix) else {
            return false;
        };
        let mut bytes = name.bytes();
        bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    });
    let uppercase_reference = value.len() >= 2
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_uppercase())
        && value
            .bytes()
            .skip(1)
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
    let placeholder = value
        .strip_prefix('<')
        .and_then(|text| text.strip_suffix('>'))
        .is_some_and(|inside| !inside.is_empty() && !inside.contains('>'));
    value.is_empty()
        || value == "[REDACTED]"
        || value.starts_with("vault:")
        || value.starts_with("${")
        || environment_reference
        || uppercase_reference
        || placeholder
}

fn is_binary(file: &Path) -> Result<bool, Failure> {
    let mut input = File::open(file)?;
    let mut sample = [0u8; 8192];
    let count = input.read(&mut sample)?;
    Ok(sample[..count].contains(&0))
}

pub fn scan_secrets(root: &Path) -> Result<Value, Failure> {
    let display_root = root.to_string_lossy().into_owned();
    let root = absolute(root)?;
    if !root.is_dir() {
        return Err(Failure::invalid(
            "evidence.secret_scan",
            format!("secret scan root is not a directory: {display_root}"),
        ));
    }
    let rules = [
        ("private-key", Regex::new(r"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----").map_err(|e| Failure::config("evidence.secret_scan", e.to_string()))?, 0usize),
        ("aws-access-key", Regex::new(r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b").map_err(|e| Failure::config("evidence.secret_scan", e.to_string()))?, 0),
        ("github-token", Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{30,}\b").map_err(|e| Failure::config("evidence.secret_scan", e.to_string()))?, 0),
        ("slack-token", Regex::new(r"\bxox[baprs]-[A-Za-z0-9-]{20,}\b").map_err(|e| Failure::config("evidence.secret_scan", e.to_string()))?, 0),
        ("jwt", Regex::new(r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b").map_err(|e| Failure::config("evidence.secret_scan", e.to_string()))?, 0),
        ("assigned-secret", Regex::new(r#"(?i)(?:token|secret|password|api[_-]?key|authorization|cookie)["']?\s*[:=]\s*["']?([A-Za-z0-9+/_=.:-]{8,})"#).map_err(|e| Failure::config("evidence.secret_scan", e.to_string()))?, 1),
    ];
    let mut findings = Vec::new();
    let mut scanned_files = 0usize;
    let mut skipped_binary = 0usize;
    let mut skipped_generated = 0usize;
    for file in files_below(&root, false)? {
        let relative = file
            .strip_prefix(&root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        if relative.starts_with("html-report/trace/assets/") {
            skipped_generated += 1;
            continue;
        }
        if is_binary(&file)? {
            skipped_binary += 1;
            continue;
        }
        scanned_files += 1;
        let mut input = BufReader::new(File::open(&file)?);
        let mut line_bytes = Vec::new();
        let mut line_number = 0usize;
        loop {
            line_bytes.clear();
            if input.read_until(b'\n', &mut line_bytes)? == 0 {
                break;
            }
            if line_bytes.last() == Some(&b'\n') {
                line_bytes.pop();
            }
            if line_bytes.last() == Some(&b'\r') {
                line_bytes.pop();
            }
            line_number += 1;
            let line = String::from_utf8_lossy(&line_bytes);
            for (rule, regex, capture) in &rules {
                for matched in regex.captures_iter(&line) {
                    let Some(full) = matched.get(0) else { continue };
                    let value = matched
                        .get(*capture)
                        .map(|part| part.as_str())
                        .unwrap_or_default();
                    if acceptable_secret(value) {
                        continue;
                    }
                    let column = line[..full.start()].encode_utf16().count() + 1;
                    findings.push(json!({
                        "rule": rule,
                        "file": relative,
                        "line": line_number,
                        "column": column,
                        "fingerprintSha256": sha256_bytes(value.as_bytes()),
                    }));
                    if findings.len() >= 1000 {
                        break;
                    }
                }
                if findings.len() >= 1000 {
                    break;
                }
            }
            if findings.len() >= 1000 {
                break;
            }
        }
        if findings.len() >= 1000 {
            break;
        }
    }
    Ok(json!({
        "schemaVersion": 1,
        "kind": "probierz-secret-scan",
        "root": root.to_string_lossy(),
        "scannedAt": now_iso(),
        "scannedFiles": scanned_files,
        "skippedBinary": skipped_binary,
        "skippedGenerated": skipped_generated,
        "passed": findings.is_empty(),
        "findings": findings,
    }))
}

pub fn secret_scan(root: Option<&Path>) -> Answer {
    let root = root
        .ok_or_else(|| Failure::invalid("evidence.secret_scan", "secret-scan needs a directory"))?;
    let result = scan_secrets(root)?;
    print_json(&result)?;
    if result.get("passed").and_then(Value::as_bool) != Some(true) {
        std::process::exit(1);
    }
    Ok(())
}

fn assert_no_secrets(root: &Path) -> Result<Value, Failure> {
    let result = scan_secrets(root)?;
    let report = root.join("diagnostics").join("secret-scan.json");
    if let Some(parent) = report.parent() {
        fs::create_dir_all(parent)?;
    }
    write_json(&report, &result)?;
    if result.get("passed").and_then(Value::as_bool) != Some(true) {
        let count = result
            .get("findings")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        return Err(Failure::invalid(
            "evidence.secret_scan",
            format!("secret scan failed with {count} finding(s)"),
        ));
    }
    Ok(result)
}

#[cfg(unix)]
fn file_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    metadata.mode() & 0o777
}

#[cfg(not(unix))]
fn file_mode(_metadata: &fs::Metadata) -> u32 {
    0o600
}

#[cfg(unix)]
fn apply_mode(path: &Path, mode: u32) -> Result<(), Failure> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn apply_mode(_path: &Path, _mode: u32) -> Result<(), Failure> {
    Ok(())
}

fn key_from_file(path: Option<&Path>) -> Result<[u8; 32], Failure> {
    let path = path
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE").map(PathBuf::from))
        .ok_or_else(|| {
            Failure::config(
                "evidence.protect",
                "artifact encryption key file is required",
            )
        })?;
    let raw = fs::read(path)?;
    let decoded = if raw.len() == 32 {
        raw
    } else {
        let text = String::from_utf8_lossy(&raw).trim().to_string();
        if text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            hex::decode(text)
                .map_err(|error| Failure::config("evidence.protect", error.to_string()))?
        } else {
            BASE64
                .decode(text)
                .map_err(|error| Failure::config("evidence.protect", error.to_string()))?
        }
    };
    decoded.try_into().map_err(|_| {
        Failure::config(
            "evidence.protect",
            "artifact encryption key must contain exactly 32 bytes, hex, or base64",
        )
    })
}

fn retention_days(document: &Value, kind: &str) -> Result<f64, Failure> {
    let name = match kind {
        "pull-request" => "pullRequestDays",
        "nightly" => "nightlyDays",
        "release" => "releaseDays",
        "synthetic" => "syntheticDays",
        _ => "adhocDays",
    };
    let retain = document.pointer("/artifacts/retain");
    let value = retain
        .and_then(|item| item.get(name))
        .or_else(|| retain.and_then(|item| item.get("pullRequestDays")))
        .and_then(|item| item.as_f64().or_else(|| item.as_i64().map(|n| n as f64)))
        .unwrap_or(14.0);
    if !value.is_finite() || value <= 0.0 {
        return Err(Failure::config(
            "evidence.retention",
            format!("invalid artifact retention for {kind}"),
        ));
    }
    Ok(value)
}

fn expires_at(started_at: &str, days: f64) -> Result<String, Failure> {
    let parsed = DateTime::parse_from_rfc3339(started_at).map_err(|_| {
        Failure::invalid(
            "evidence.retention",
            format!("invalid run timestamp: {started_at}"),
        )
    })?;
    let milliseconds = (days * 86_400_000.0) as i64;
    Ok((parsed + chrono::Duration::milliseconds(milliseconds))
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn encoded_header(header: &Value) -> Result<Vec<u8>, Failure> {
    let body = format!("{}\n", serde_json::to_string(header)?).into_bytes();
    let length: u32 = body.len().try_into().map_err(|_| {
        Failure::invalid("evidence.protect", "invalid evidence bundle header length")
    })?;
    let mut prefix = Vec::with_capacity(MAGIC.len() + 4 + body.len());
    prefix.extend_from_slice(MAGIC);
    prefix.extend_from_slice(&length.to_be_bytes());
    prefix.extend_from_slice(&body);
    Ok(prefix)
}

fn read_header(file: &Path) -> Result<(Value, usize, Vec<u8>), Failure> {
    let mut input = File::open(file)?;
    let mut prefix = vec![0u8; MAGIC.len() + 4];
    input.read_exact(&mut prefix).map_err(|_| {
        Failure::invalid(
            "evidence.restore",
            "not a Probierz encrypted evidence bundle",
        )
    })?;
    if &prefix[..MAGIC.len()] != MAGIC {
        return Err(Failure::invalid(
            "evidence.restore",
            "not a Probierz encrypted evidence bundle",
        ));
    }
    let length = u32::from_be_bytes(prefix[MAGIC.len()..].try_into().map_err(|_| {
        Failure::invalid("evidence.restore", "invalid evidence bundle header length")
    })?) as usize;
    if length == 0 || length > 1024 * 1024 {
        return Err(Failure::invalid(
            "evidence.restore",
            "invalid evidence bundle header length",
        ));
    }
    let mut body = vec![0u8; length];
    input
        .read_exact(&mut body)
        .map_err(|_| Failure::invalid("evidence.restore", "truncated evidence bundle header"))?;
    let header: Value = serde_json::from_slice(&body)?;
    prefix.extend_from_slice(&body);
    Ok((header, prefix.len(), prefix))
}

fn remove_plaintext_source(
    source: &Path,
    manifest_path: &Path,
    retention_kind: &str,
    protected: &Value,
) -> Result<(), Failure> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name() == "run-manifest.json" {
            continue;
        }
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    let mut current = json_file(manifest_path)?;
    let object = current
        .as_object_mut()
        .ok_or_else(|| Failure::config("evidence.protect", "run manifest is not an object"))?;
    if object.get("kind").is_none_or(Value::is_null) {
        object.insert("kind".into(), json!(retention_kind));
    }
    let mut protection = protected.clone();
    protection
        .as_object_mut()
        .ok_or_else(|| Failure::config("evidence.protect", "protection is not an object"))?
        .insert("plaintextRemoved".into(), json!(true));
    object.insert("protection".into(), protection);
    object.insert("plaintextArtifactsRemovedAt".into(), json!(now_iso()));
    let temporary = manifest_path.with_extension(format!(
        "json.tmp-{}-{}",
        std::process::id(),
        Utc::now().timestamp_millis()
    ));
    write_new_json(&temporary, &current, true)?;
    fs::rename(temporary, manifest_path)?;
    Ok(())
}

type Aes256Ctr = Ctr32BE<Aes256>;

fn ghash_feed(state: &mut GHash, tail: &mut Vec<u8>, mut bytes: &[u8]) {
    if !tail.is_empty() {
        let needed = 16 - tail.len();
        let take = needed.min(bytes.len());
        tail.extend_from_slice(&bytes[..take]);
        bytes = &bytes[take..];
        if tail.len() == 16 {
            let block = *ghash::Block::from_slice(tail);
            state.update(&[block]);
            tail.clear();
        }
    }
    while bytes.len() >= 16 {
        let block = *ghash::Block::from_slice(&bytes[..16]);
        state.update(&[block]);
        bytes = &bytes[16..];
    }
    tail.extend_from_slice(bytes);
}

fn ghash_pad(state: &mut GHash, tail: &mut Vec<u8>) {
    if tail.is_empty() {
        return;
    }
    tail.resize(16, 0);
    let block = *ghash::Block::from_slice(tail);
    state.update(&[block]);
    tail.clear();
}

fn gcm_state(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    point: &'static str,
) -> Result<(Aes256Ctr, GHash, [u8; 16]), Failure> {
    if nonce.len() != 12 {
        return Err(Failure::invalid(
            point,
            "encrypted evidence nonce is invalid",
        ));
    }
    let aes =
        Aes256::new_from_slice(key).map_err(|error| Failure::config(point, error.to_string()))?;
    let mut hash_key = aes::cipher::Block::<Aes256>::default();
    aes.encrypt_block(&mut hash_key);
    let mut state = GHash::new(ghash::Key::from_slice(&hash_key));
    let mut aad_tail = Vec::with_capacity(16);
    ghash_feed(&mut state, &mut aad_tail, aad);
    ghash_pad(&mut state, &mut aad_tail);

    let mut initial_counter = [0u8; 16];
    initial_counter[..12].copy_from_slice(nonce);
    initial_counter[15] = 2;
    let stream = Aes256Ctr::new_from_slices(key, &initial_counter)
        .map_err(|error| Failure::config(point, error.to_string()))?;

    initial_counter[15] = 1;
    let mut tag_mask = aes::cipher::Block::<Aes256>::clone_from_slice(&initial_counter);
    aes.encrypt_block(&mut tag_mask);
    let mut mask = [0u8; 16];
    mask.copy_from_slice(&tag_mask);
    Ok((stream, state, mask))
}

fn finish_gcm_tag(
    mut state: GHash,
    tail: &mut Vec<u8>,
    mask: &[u8; 16],
    aad_bytes: usize,
    ciphertext_bytes: u64,
    point: &'static str,
) -> Result<[u8; 16], Failure> {
    ghash_pad(&mut state, tail);
    let aad_bits = u64::try_from(aad_bytes)
        .ok()
        .and_then(|value| value.checked_mul(8))
        .ok_or_else(|| Failure::invalid(point, "encrypted evidence header is too large"))?;
    let ciphertext_bits = ciphertext_bytes
        .checked_mul(8)
        .ok_or_else(|| Failure::invalid(point, "encrypted evidence payload is too large"))?;
    let mut lengths = [0u8; 16];
    lengths[..8].copy_from_slice(&aad_bits.to_be_bytes());
    lengths[8..].copy_from_slice(&ciphertext_bits.to_be_bytes());
    state.update(&[*ghash::Block::from_slice(&lengths)]);
    let mut tag = state.finalize();
    for (byte, mask_byte) in tag.iter_mut().zip(mask) {
        *byte ^= mask_byte;
    }
    let mut result = [0u8; 16];
    result.copy_from_slice(&tag);
    Ok(result)
}

fn encrypt_chunk(
    output: &mut File,
    stream: &mut Aes256Ctr,
    state: &mut GHash,
    tail: &mut Vec<u8>,
    bytes: &mut [u8],
) -> Result<(), Failure> {
    stream.try_apply_keystream(bytes).map_err(|_| {
        Failure::invalid(
            "evidence.protect",
            "encrypted evidence payload is too large",
        )
    })?;
    ghash_feed(state, tail, bytes);
    output.write_all(bytes)?;
    Ok(())
}

fn encrypt_bundle_payload(
    output: &mut File,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    index: &[u8],
    source_files: &[PathBuf],
) -> Result<[u8; 16], Failure> {
    let (mut stream, mut state, mask) = gcm_state(key, nonce, aad, "evidence.protect")?;
    let mut tail = Vec::with_capacity(16);
    let mut prefix = Vec::with_capacity(4 + index.len());
    prefix.extend_from_slice(&(index.len() as u32).to_be_bytes());
    prefix.extend_from_slice(index);
    let mut ciphertext_bytes = prefix.len() as u64;
    encrypt_chunk(output, &mut stream, &mut state, &mut tail, &mut prefix)?;

    let mut buffer = vec![0u8; 128 * 1024];
    for file in source_files {
        let mut input = File::open(file)?;
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            ciphertext_bytes = ciphertext_bytes.checked_add(count as u64).ok_or_else(|| {
                Failure::invalid(
                    "evidence.protect",
                    "encrypted evidence payload is too large",
                )
            })?;
            encrypt_chunk(
                output,
                &mut stream,
                &mut state,
                &mut tail,
                &mut buffer[..count],
            )?;
        }
    }
    finish_gcm_tag(
        state,
        &mut tail,
        &mask,
        aad.len(),
        ciphertext_bytes,
        "evidence.protect",
    )
}

struct TemporaryPlaintext(PathBuf);

impl TemporaryPlaintext {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryPlaintext {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn decrypt_bundle_payload(
    file: &Path,
    destination: &Path,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    offset: usize,
) -> Result<(TemporaryPlaintext, u64), Failure> {
    let total_bytes = fs::metadata(file)?.len();
    let overhead = (offset as u64)
        .checked_add(TAG_BYTES as u64)
        .ok_or_else(|| {
            Failure::invalid("evidence.restore", "truncated encrypted evidence bundle")
        })?;
    let ciphertext_bytes = total_bytes.checked_sub(overhead).ok_or_else(|| {
        Failure::invalid("evidence.restore", "truncated encrypted evidence bundle")
    })?;
    let (mut stream, mut state, mask) = gcm_state(key, nonce, aad, "evidence.restore")?;
    let mut tail = Vec::with_capacity(16);
    let temporary = TemporaryPlaintext(
        destination
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!(
                ".probierz-restore-{}-{}",
                std::process::id(),
                Utc::now().timestamp_millis(),
            )),
    );
    let mut plaintext = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary.path())?;
    apply_mode(temporary.path(), 0o600)?;
    let mut input = File::open(file)?;
    input.seek(SeekFrom::Start(offset as u64))?;
    let mut encrypted = (&mut input).take(ciphertext_bytes);
    let mut buffer = vec![0u8; 128 * 1024];
    loop {
        let count = encrypted.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        ghash_feed(&mut state, &mut tail, &buffer[..count]);
        stream
            .try_apply_keystream(&mut buffer[..count])
            .map_err(|_| {
                Failure::invalid(
                    "evidence.restore",
                    "encrypted evidence payload is too large",
                )
            })?;
        plaintext.write_all(&buffer[..count])?;
    }
    plaintext.flush()?;
    drop(plaintext);
    drop(encrypted);

    let mut stored_tag = [0u8; TAG_BYTES];
    input
        .read_exact(&mut stored_tag)
        .map_err(|_| Failure::invalid("evidence.restore", "truncated encrypted evidence bundle"))?;
    let computed_tag = finish_gcm_tag(
        state,
        &mut tail,
        &mask,
        aad.len(),
        ciphertext_bytes,
        "evidence.restore",
    )?;
    if computed_tag.ct_eq(&stored_tag).unwrap_u8() != 1 {
        return Err(Failure::invalid(
            "evidence.restore",
            "encrypted evidence authentication failed: Unsupported state or unable to authenticate data",
        ));
    }
    Ok((temporary, ciphertext_bytes))
}

pub fn protect(
    harness: &Path,
    app_id: Option<&str>,
    run_id: Option<&str>,
    kind: Option<&str>,
    key_file: Option<&Path>,
    remove_source: bool,
) -> Answer {
    let app_id = app_id.ok_or_else(|| {
        Failure::invalid("evidence.protect", "protect needs an app ID and run ID")
    })?;
    let run_id = run_id.ok_or_else(|| {
        Failure::invalid("evidence.protect", "protect needs an app ID and run ID")
    })?;
    let result = protect_run(harness, app_id, run_id, kind, key_file, remove_source);
    match result {
        Ok(value) => {
            let resource = value.get("file").and_then(Value::as_str).map(Path::new);
            let _ = audit_access(
                harness,
                "artifact.protect",
                "allowed",
                Some(app_id),
                Some(run_id),
                resource,
                json!({ "removePlaintext": remove_source, "bundleSha256": value.get("sha256").cloned().unwrap_or(Value::Null) }),
            );
            print_json(&value)
        }
        Err(error) => {
            let _ = audit_access(
                harness,
                "artifact.protect",
                "denied",
                Some(app_id),
                Some(run_id),
                None,
                json!({ "error": error.detail }),
            );
            Err(error)
        }
    }
}

pub(crate) fn protect_run(
    harness: &Path,
    app_id: &str,
    run_id: &str,
    kind: Option<&str>,
    key_file: Option<&Path>,
    remove_source: bool,
) -> Result<Value, Failure> {
    let run = get_run(harness, app_id, run_id)?;
    let manifest_path = PathBuf::from(
        run.get("manifestPath")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let source = manifest_path
        .parent()
        .ok_or_else(|| Failure::invalid("evidence.protect", "run manifest has no directory"))?;
    let source_root = absolute(&harness.join("test-results").join(app_id))?;
    let absolute_source = absolute(source)?;
    if !absolute_source.starts_with(&source_root) {
        return Err(Failure::invalid(
            "evidence.protect",
            "run is outside its product artifact root",
        ));
    }
    let app = manifest::load(harness, app_id)?;
    let document = yaml_json(&app.document)?;
    let retention_kind = kind
        .or_else(|| run.get("kind").and_then(Value::as_str))
        .unwrap_or("adhoc");
    let days = retention_days(&document, retention_kind)?;
    let key = key_from_file(key_file)?;
    let current = json_file(&manifest_path)?;
    if current
        .pointer("/protection/plaintextRemoved")
        .and_then(Value::as_bool)
        == Some(true)
    {
        let mut protected = current
            .get("protection")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let file = PathBuf::from(
            protected
                .get("file")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        if !file.exists() {
            return Err(Failure::invalid(
                "evidence.protect",
                "plaintext was removed but the encrypted evidence bundle is missing",
            ));
        }
        if protected
            .get("keyFingerprintSha256")
            .and_then(Value::as_str)
            != Some(&sha256_bytes(&key))
        {
            return Err(Failure::invalid(
                "evidence.protect",
                "artifact encryption key fingerprint mismatch",
            ));
        }
        protected
            .as_object_mut()
            .unwrap()
            .insert("reused".into(), json!(true));
        return Ok(protected);
    }
    let destination = harness
        .join("test-results")
        .join(".protected")
        .join(app_id)
        .join(retention_kind)
        .join(format!("{run_id}.pev"));
    let existing = destination.exists();
    let scan = if existing {
        None
    } else {
        Some(assert_no_secrets(source)?)
    };
    let source_files = files_below(source, true)?;
    let mut entries = Vec::with_capacity(source_files.len());
    for file in &source_files {
        let metadata = fs::metadata(file)?;
        entries.push(json!({
            "file": file.strip_prefix(source).unwrap_or(file).to_string_lossy().replace('\\', "/"),
            "bytes": metadata.len(),
            "mode": file_mode(&metadata),
            "sha256": sha256_file(file)?,
        }));
    }
    let index = format!(
        "{}\n",
        serde_json::to_string(&json!({ "schemaVersion": 1, "runId": run_id, "files": entries }))?
    )
    .into_bytes();
    let index_hash = sha256_bytes(&index);
    if existing {
        let (header, _, _) = read_header(&destination)?;
        if header.get("runId").and_then(Value::as_str) != Some(run_id)
            || header.get("appId").and_then(Value::as_str) != Some(app_id)
        {
            return Err(Failure::invalid(
                "evidence.protect",
                "encrypted bundle identity mismatch",
            ));
        }
        if header.get("keyFingerprintSha256").and_then(Value::as_str) != Some(&sha256_bytes(&key)) {
            return Err(Failure::invalid(
                "evidence.protect",
                "artifact encryption key fingerprint mismatch",
            ));
        }
        let Some(existing_index) = header.get("contentIndexSha256").and_then(Value::as_str) else {
            return Err(Failure::invalid(
                "evidence.protect",
                "existing encrypted bundle predates source-integrity metadata",
            ));
        };
        if existing_index != index_hash {
            return Err(Failure::invalid(
                "evidence.protect",
                "plaintext artifacts changed after the encrypted bundle was created",
            ));
        }
        let protected = json!({
            "file": destination.to_string_lossy(),
            "bytes": fs::metadata(&destination)?.len(),
            "sha256": sha256_file(&destination)?,
            "contentIndexSha256": existing_index,
            "keyFingerprintSha256": header.get("keyFingerprintSha256").cloned().unwrap_or(Value::Null),
            "expiresAt": header.get("expiresAt").cloned().unwrap_or(Value::Null),
            "retentionDays": header.get("retentionDays").cloned().unwrap_or(Value::Null),
            "files": header.get("files").cloned().unwrap_or(Value::Null),
            "secretScan": header.get("secretScan").cloned().unwrap_or(Value::Null),
            "plaintextRemoved": remove_source,
            "reused": true,
        });
        if remove_source {
            remove_plaintext_source(source, &manifest_path, retention_kind, &protected)?;
        }
        return Ok(protected);
    }
    let scan = scan.unwrap_or_else(|| json!({}));
    let primary = run
        .pointer("/source/repositories")
        .and_then(Value::as_array)
        .and_then(|repositories| {
            repositories
                .iter()
                .find(|entry| entry.get("index").and_then(Value::as_i64) == Some(0))
                .or_else(|| repositories.first())
        });
    let journeys = run
        .get("journeys")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let journey_manifest = document.get("journeys").and_then(Value::as_object);
    let journey_identities = journeys.iter().filter_map(|name| {
        let name = name.as_str()?;
        let journey = journey_manifest?.get(name)?;
        journey.get("journeyId")?;
        Some(json!({
            "name": name,
            "journeyId": journey.get("journeyId").cloned().unwrap_or(Value::Null),
            "journeyVersion": journey.get("journeyVersion").cloned().unwrap_or(Value::Null),
            "journeyVersionId": journey.get("journeyVersionId").cloned().unwrap_or(Value::Null),
            "firstSuccessFact": journey.get("firstSuccessFact").cloned().unwrap_or(Value::Null),
            "screenId": journey.pointer("/publication/screenId").cloned().unwrap_or(Value::Null),
        }))
    }).collect::<Vec<_>>();
    let evidence_level = if run.get("status").and_then(Value::as_str) != Some("passed") {
        "E0"
    } else if run.pointer("/conditions/record").and_then(Value::as_bool) == Some(true)
        && run.pointer("/evidence/report").and_then(Value::as_bool) == Some(true)
        && run.pointer("/evidence/analysis").and_then(Value::as_bool) == Some(true)
        && run
            .pointer("/evidence/capturePresent")
            .and_then(Value::as_bool)
            == Some(true)
    {
        "E3"
    } else {
        "E2"
    };
    let started = run
        .get("completedAt")
        .or_else(|| run.get("startedAt"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let header = json!({
        "schemaVersion": 2,
        "kind": "probierz-encrypted-evidence",
        "algorithm": "AES-256-GCM",
        "appId": app_id,
        "runId": run_id,
        "attemptId": run_id,
        "productId": document.get("productId").cloned().unwrap_or_else(|| json!(app_id)),
        "releaseVersion": current.get("appVersion").cloned().filter(|value| !value.is_null()).or_else(|| current.pointer("/conditions/PROBIERZ_RELEASE").cloned()).unwrap_or(Value::Null),
        "sourceRevision": primary.and_then(|value| value.get("gitSha")).cloned().unwrap_or(Value::Null),
        "sourceSha256": run.pointer("/source/sha256").cloned().unwrap_or(Value::Null),
        "buildSha256": run.pointer("/build/sha256").cloned().unwrap_or(Value::Null),
        "evidenceLevel": evidence_level,
        "journeys": journey_identities,
        "runKind": retention_kind,
        "createdAt": now_iso(),
        "expiresAt": expires_at(started, days)?,
        "retentionDays": js_number(days),
        "pii": document.pointer("/artifacts/pii").cloned().unwrap_or_else(|| json!("unknown")),
        "nonce": BASE64.encode(nonce),
        "keyFingerprintSha256": sha256_bytes(&key),
        "contentIndexSha256": index_hash,
        "secretScan": {
            "passed": scan.get("passed").cloned().unwrap_or(Value::Null),
            "scannedFiles": scan.get("scannedFiles").cloned().unwrap_or(Value::Null),
            "skippedBinary": scan.get("skippedBinary").cloned().unwrap_or(Value::Null),
        },
        "files": entries.len(),
        "plaintextBytes": entries.iter().filter_map(|entry| entry.get("bytes").and_then(Value::as_u64)).sum::<u64>(),
    });
    let prefix = encoded_header(&header)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
        apply_mode(parent, 0o700)?;
    }
    let temporary = PathBuf::from(format!(
        "{}.tmp-{}-{}",
        destination.display(),
        std::process::id(),
        Utc::now().timestamp_millis()
    ));
    let writing = (|| -> Result<(), Failure> {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        output.write_all(&prefix)?;
        let tag =
            encrypt_bundle_payload(&mut output, &key, &nonce, &prefix, &index, &source_files)?;
        output.write_all(&tag)?;
        output.flush()?;
        drop(output);
        apply_mode(&temporary, 0o600)?;
        fs::rename(&temporary, &destination)?;
        Ok(())
    })();
    if let Err(error) = writing {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    let protected = json!({
        "file": destination.to_string_lossy(),
        "bytes": fs::metadata(&destination)?.len(),
        "sha256": sha256_file(&destination)?,
        "contentIndexSha256": header.get("contentIndexSha256").cloned().unwrap_or(Value::Null),
        "keyFingerprintSha256": header.get("keyFingerprintSha256").cloned().unwrap_or(Value::Null),
        "expiresAt": header.get("expiresAt").cloned().unwrap_or(Value::Null),
        "retentionDays": js_number(days),
        "files": entries.len(),
        "secretScan": header.get("secretScan").cloned().unwrap_or(Value::Null),
        "plaintextRemoved": remove_source,
    });
    if remove_source {
        remove_plaintext_source(source, &manifest_path, retention_kind, &protected)?;
    }
    Ok(protected)
}

pub fn restore(
    harness: &Path,
    file: Option<&Path>,
    destination: Option<&Path>,
    key_file: Option<&Path>,
) -> Answer {
    let file = file.ok_or_else(|| {
        Failure::invalid("evidence.restore", "restore needs a bundle and destination")
    })?;
    let destination = destination.ok_or_else(|| {
        Failure::invalid("evidence.restore", "restore needs a bundle and destination")
    })?;
    let result = restore_bundle(file, destination, key_file);
    match result {
        Ok(value) => {
            let app = value.get("appId").and_then(Value::as_str);
            let run = value.get("runId").and_then(Value::as_str);
            let _ = audit_access(
                harness,
                "artifact.restore",
                "allowed",
                app,
                run,
                Some(file),
                json!({
                    "destination": value.get("destination").cloned().unwrap_or(Value::Null),
                    "files": value.get("files").cloned().unwrap_or(Value::Null),
                }),
            );
            print_json(&value)
        }
        Err(error) => {
            let _ = audit_access(
                harness,
                "artifact.restore",
                "denied",
                None,
                None,
                Some(file),
                json!({
                    "destination": destination.to_string_lossy(), "error": error.detail,
                }),
            );
            Err(error)
        }
    }
}

fn restore_bundle(
    file: &Path,
    destination: &Path,
    key_file: Option<&Path>,
) -> Result<Value, Failure> {
    if destination.is_dir() && fs::read_dir(destination)?.next().is_some() {
        return Err(Failure::invalid(
            "evidence.restore",
            "restore destination must be empty",
        ));
    }
    fs::create_dir_all(destination)?;
    apply_mode(destination, 0o700)?;
    let key = key_from_file(key_file)?;
    let (header, offset, aad) = read_header(file)?;
    if header.get("algorithm").and_then(Value::as_str) != Some("AES-256-GCM") {
        return Err(Failure::invalid(
            "evidence.restore",
            format!(
                "unsupported evidence algorithm: {}",
                header
                    .get("algorithm")
                    .and_then(Value::as_str)
                    .unwrap_or("undefined")
            ),
        ));
    }
    if header.get("keyFingerprintSha256").and_then(Value::as_str) != Some(&sha256_bytes(&key)) {
        return Err(Failure::invalid(
            "evidence.restore",
            "artifact encryption key fingerprint mismatch",
        ));
    }
    let nonce_bytes = BASE64
        .decode(
            header
                .get("nonce")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .map_err(|error| Failure::invalid("evidence.restore", error.to_string()))?;
    let (payload, plaintext_bytes) =
        decrypt_bundle_payload(file, destination, &key, &nonce_bytes, &aad, offset)?;
    let mut plaintext = File::open(payload.path())?;
    let mut index_size = [0u8; 4];
    plaintext
        .read_exact(&mut index_size)
        .map_err(|_| Failure::invalid("evidence.restore", "truncated evidence index"))?;
    let length = u32::from_be_bytes(index_size) as usize;
    if length == 0 || length > 64 * 1024 * 1024 {
        return Err(Failure::invalid(
            "evidence.restore",
            "invalid evidence index length",
        ));
    }
    let mut index_bytes = vec![0u8; length];
    plaintext
        .read_exact(&mut index_bytes)
        .map_err(|_| Failure::invalid("evidence.restore", "truncated evidence index"))?;
    let index: Value = serde_json::from_slice(&index_bytes)?;
    let entries = index
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let root = absolute(destination)?;
    let mut cursor = 4u64 + length as u64;
    let mut buffer = vec![0u8; 128 * 1024];
    for entry in &entries {
        let member = entry
            .get("file")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if member.is_empty()
            || Path::new(member).is_absolute()
            || member.split('/').any(|part| part == "..")
        {
            return Err(Failure::invalid(
                "evidence.restore",
                format!("unsafe evidence member: {member}"),
            ));
        }
        let output = absolute(&root.join(member))?;
        if !output.starts_with(&root) {
            return Err(Failure::invalid(
                "evidence.restore",
                format!("unsafe evidence member: {member}"),
            ));
        }
        let count = entry.get("bytes").and_then(Value::as_u64).unwrap_or(0);
        cursor = cursor.checked_add(count).ok_or_else(|| {
            Failure::invalid(
                "evidence.restore",
                "encrypted evidence payload has trailing or missing bytes",
            )
        })?;
        if cursor > plaintext_bytes {
            return Err(Failure::invalid(
                "evidence.restore",
                "encrypted evidence payload has trailing or missing bytes",
            ));
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
            apply_mode(parent, 0o700)?;
        }
        let mut target = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)?;
        let mut remaining = count;
        let mut digest = Sha256::new();
        while remaining > 0 {
            let wanted = remaining.min(buffer.len() as u64) as usize;
            plaintext.read_exact(&mut buffer[..wanted]).map_err(|_| {
                Failure::invalid(
                    "evidence.restore",
                    "encrypted evidence payload has trailing or missing bytes",
                )
            })?;
            digest.update(&buffer[..wanted]);
            target.write_all(&buffer[..wanted])?;
            remaining -= wanted as u64;
        }
        drop(target);
        apply_mode(
            &output,
            entry
                .get("mode")
                .and_then(Value::as_u64)
                .filter(|mode| *mode != 0)
                .unwrap_or(0o600) as u32,
        )?;
        if hex::encode(digest.finalize())
            != entry
                .get("sha256")
                .and_then(Value::as_str)
                .unwrap_or_default()
        {
            return Err(Failure::invalid(
                "evidence.restore",
                format!("restored evidence hash mismatch: {member}"),
            ));
        }
    }
    if cursor != plaintext_bytes {
        return Err(Failure::invalid(
            "evidence.restore",
            "encrypted evidence payload has trailing or missing bytes",
        ));
    }
    Ok(json!({
        "appId": header.get("appId").cloned().unwrap_or(Value::Null),
        "runId": header.get("runId").cloned().unwrap_or(Value::Null),
        "destination": destination.to_string_lossy(),
        "files": entries.len(),
        "authenticated": true,
    }))
}

pub fn retention(harness: &Path, app_id: Option<&str>, at: Option<&str>, apply: bool) -> Answer {
    let app_id = app_id
        .ok_or_else(|| Failure::invalid("evidence.retention", "retention needs an app ID"))?;
    let at = match at {
        Some(value) => DateTime::parse_from_rfc3339(value)
            .map_err(|_| Failure::invalid("evidence.retention", "invalid retention time"))?
            .with_timezone(&Utc),
        None => Utc::now(),
    };
    let app = manifest::load(harness, app_id)?;
    let document = yaml_json(&app.document)?;
    let mut items = Vec::new();
    for file in manifests_below(&harness.join("test-results").join(app_id))? {
        let run = json_file(&file)?;
        let kind = run.get("kind").and_then(Value::as_str).unwrap_or("adhoc");
        let expiry = expires_at(
            run.get("completedAt")
                .or_else(|| run.get("startedAt"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            retention_days(&document, kind)?,
        )?;
        let expired = DateTime::parse_from_rfc3339(&expiry).is_ok_and(|value| value <= at);
        items.push(json!({
            "type": "run", "appId": app_id, "runId": run.get("runId").cloned().unwrap_or(Value::Null),
            "kind": kind, "path": file.parent().unwrap_or(&file).to_string_lossy(), "expiresAt": expiry, "expired": expired,
        }));
    }
    let protected = harness.join("test-results").join(".protected").join(app_id);
    if protected.exists() {
        for file in files_below(&protected, false)?
            .into_iter()
            .filter(|file| file.extension().and_then(|value| value.to_str()) == Some("pev"))
        {
            let (header, _, _) = read_header(&file)?;
            let expiry = header
                .get("expiresAt")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let expired = DateTime::parse_from_rfc3339(expiry).is_ok_and(|value| value <= at);
            items.push(json!({
                "type": "protected", "appId": app_id, "runId": header.get("runId").cloned().unwrap_or(Value::Null),
                "kind": header.get("runKind").cloned().unwrap_or(Value::Null), "path": file.to_string_lossy(),
                "expiresAt": expiry, "expired": expired,
            }));
        }
    }
    items.sort_by(|left, right| {
        left.get("expiresAt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(
                right
                    .get("expiresAt")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
            .then_with(|| {
                left.get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .cmp(
                        right
                            .get("path")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    )
            })
    });
    let expired = items
        .iter()
        .filter(|item| item.get("expired").and_then(Value::as_bool) == Some(true))
        .count();
    let mut removed = Vec::new();
    if apply {
        for item in items
            .iter()
            .filter(|item| item.get("expired").and_then(Value::as_bool) == Some(true))
        {
            let path = PathBuf::from(item.get("path").and_then(Value::as_str).unwrap_or_default());
            if item.get("type").and_then(Value::as_str) == Some("run") {
                let _ = fs::remove_dir_all(&path);
            } else {
                let _ = fs::remove_file(&path);
            }
            removed.push(json!(path.to_string_lossy()));
        }
    }
    let at_text = at.to_rfc3339_opts(SecondsFormat::Millis, true);
    let _ = audit_access(
        harness,
        if apply {
            "retention.apply"
        } else {
            "retention.plan"
        },
        "allowed",
        Some(app_id),
        None,
        None,
        json!({ "expired": expired, "removed": removed.len(), "at": at_text }),
    );
    print_json(&json!({
        "schemaVersion": 1, "appId": app_id, "at": at_text, "expired": expired, "items": items,
        "applied": apply, "removed": removed,
    }))
}

fn write_json(path: &Path, value: &Value) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    apply_mode(path, 0o600)?;
    Ok(())
}

fn write_new_json(path: &Path, value: &Value, mode_600: bool) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = OpenOptions::new().write(true).create_new(true).open(path)?;
    writeln!(output, "{}", serde_json::to_string_pretty(value)?)?;
    drop(output);
    if mode_600 {
        apply_mode(path, 0o600)?;
    }
    Ok(())
}

fn segment(value: &str, fallback: &str) -> String {
    let mut clean = String::new();
    let mut replacing = false;
    for character in value.trim().chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
            clean.push(character);
            replacing = false;
        } else if !replacing {
            clean.push('-');
            replacing = true;
        }
    }
    if clean.is_empty() {
        fallback.to_string()
    } else {
        clean
    }
}

fn policy_conditions(run: &Value, document: &Value) -> Value {
    let mut names = BTreeSet::from(["PROBIERZ_RELEASE".to_string()]);
    if let Some(profiles) = document.get("matrix").and_then(Value::as_object) {
        for profile in profiles.values() {
            if let Some(dimensions) = profile.get("dimensions").and_then(Value::as_object) {
                names.extend(dimensions.keys().cloned());
            }
        }
    }
    let conditions = run.get("conditions").and_then(Value::as_object);
    Value::Object(
        names
            .into_iter()
            .filter_map(|name| {
                conditions
                    .and_then(|map| map.get(&name))
                    .cloned()
                    .map(|value| (name, value))
            })
            .collect(),
    )
}

fn signed_protection(protection: Option<&Value>) -> Value {
    let Some(value) = protection.filter(|value| !value.is_null()) else {
        return Value::Null;
    };
    json!({
        "bytes": value.get("bytes").and_then(Value::as_f64).map(js_number).unwrap_or_else(|| json!(0)),
        "sha256": value.get("sha256").cloned().unwrap_or(Value::Null),
        "contentIndexSha256": value.get("contentIndexSha256").cloned().unwrap_or(Value::Null),
        "keyFingerprintSha256": value.get("keyFingerprintSha256").cloned().unwrap_or(Value::Null),
        "plaintextRemoved": value.get("plaintextRemoved").and_then(Value::as_bool).unwrap_or(false),
        "secretScan": value.get("secretScan").cloned().unwrap_or(Value::Null),
    })
}

fn journey_identities(run: &Value, document: &Value) -> Vec<Value> {
    let Some(journeys) = document.get("journeys").and_then(Value::as_object) else {
        return Vec::new();
    };
    run.get("journeys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|name| {
            let name = name.as_str()?;
            let journey = journeys.get(name)?;
            journey.get("journeyId")?;
            Some(json!({
                "name": name,
                "journeyId": journey.get("journeyId").cloned().unwrap_or(Value::Null),
                "journeyVersion": journey.get("journeyVersion").cloned().unwrap_or(Value::Null),
                "journeyVersionId": journey.get("journeyVersionId").cloned().unwrap_or(Value::Null),
                "firstSuccessFact": journey.get("firstSuccessFact").cloned().unwrap_or(Value::Null),
                "publication": journey.get("publication").cloned().unwrap_or(Value::Null),
            }))
        })
        .collect()
}

fn signed_media(run: &Value) -> Vec<Value> {
    let root = PathBuf::from(
        run.get("manifestPath")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let root = root.parent().unwrap_or(Path::new(""));
    let artifacts: HashMap<String, Value> = run
        .get("artifacts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artifact| {
            artifact
                .get("file")
                .and_then(Value::as_str)
                .map(|file| (file.replace('\\', "/"), artifact.clone()))
        })
        .collect();
    let analysis = run
        .get("analysisPath")
        .and_then(Value::as_str)
        .and_then(|path| try_json_file(Path::new(path)));
    let mut typed = BTreeMap::<String, String>::new();
    for media in analysis
        .as_ref()
        .and_then(|value| value.get("media"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(path) = media.get("file").and_then(Value::as_str) else {
            continue;
        };
        let Ok(path) = absolute(Path::new(path)) else {
            continue;
        };
        let Ok(root_abs) = absolute(root) else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(&root_abs) else {
            continue;
        };
        let kind = match media.get("kind").and_then(Value::as_str) {
            Some("video") => Some("recording"),
            Some("screenshot") => Some("screenshot"),
            Some("trace") => Some("trace"),
            _ => None,
        };
        if let Some(kind) = kind {
            typed.insert(
                relative.to_string_lossy().replace('\\', "/"),
                kind.to_string(),
            );
        }
    }
    for file in ["report.json", "timeline.json"] {
        if artifacts.contains_key(file) {
            typed.insert(file.to_string(), "trace".to_string());
        }
    }
    typed.into_iter().filter_map(|(file, kind)| {
        let inventory = artifacts.get(&file)?;
        Some(json!({
            "file": file, "artifactKind": kind,
            "contentSha256": inventory.get("sha256").cloned().unwrap_or(Value::Null),
            "bytes": inventory.get("bytes").and_then(Value::as_f64).map(js_number).unwrap_or_else(|| json!(0)),
            "capturedAt": run.get("completedAt").or_else(|| run.get("startedAt")).cloned().unwrap_or(Value::Null),
        }))
    }).collect()
}

fn signed_receipt_run(run: &Value, document: &Value) -> Value {
    let status = run
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let evidence_level = if status != "passed" {
        "E0"
    } else if run.pointer("/conditions/record").and_then(Value::as_bool) == Some(true)
        && run.pointer("/evidence/report").and_then(Value::as_bool) == Some(true)
        && run.pointer("/evidence/analysis").and_then(Value::as_bool) == Some(true)
        && run
            .pointer("/evidence/capturePresent")
            .and_then(Value::as_bool)
            == Some(true)
    {
        "E3"
    } else {
        "E2"
    };
    let source_revision = run
        .pointer("/source/repositories")
        .and_then(Value::as_array)
        .and_then(|repositories| {
            repositories
                .iter()
                .find(|entry| entry.get("index").and_then(Value::as_i64) == Some(0))
                .or_else(|| repositories.first())
                .and_then(|entry| entry.get("gitSha"))
                .cloned()
        })
        .unwrap_or(Value::Null);
    let artifacts = run.get("artifacts").and_then(Value::as_array).into_iter().flatten().map(|artifact| json!({
        "file": artifact.get("file").cloned().unwrap_or(Value::Null),
        "sha256": artifact.get("sha256").cloned().unwrap_or(Value::Null),
        "bytes": artifact.get("bytes").and_then(Value::as_f64).map(js_number).unwrap_or_else(|| json!(0)),
    })).collect::<Vec<_>>();
    json!({
        "runId": run.get("runId").cloned().unwrap_or(Value::Null),
        "target": run.get("target").cloned().unwrap_or(Value::Null),
        "spec": run.get("spec").cloned().unwrap_or(Value::Null),
        "journeys": run.get("journeys").cloned().unwrap_or_else(|| json!([])),
        "journeyIdentities": journey_identities(run, document),
        "status": status,
        "evidenceLevel": evidence_level,
        "kind": run.get("kind").cloned().unwrap_or(Value::Null),
        "harness": run.get("harness").cloned().unwrap_or(Value::Null),
        "source": run.get("source").cloned().unwrap_or(Value::Null),
        "build": run.get("build").cloned().unwrap_or(Value::Null),
        "sourceRevision": source_revision,
        "device": run.get("device").cloned().unwrap_or(Value::Null),
        "startedAt": run.get("startedAt").cloned().unwrap_or(Value::Null),
        "completedAt": run.get("completedAt").cloned().unwrap_or(Value::Null),
        "conditions": policy_conditions(run, document),
        "protection": signed_protection(run.get("protection")),
        "artifacts": artifacts,
        "media": signed_media(run),
    })
}
pub(crate) fn signed_receipt_run_value(run: &Value, document: &Value) -> Value {
    signed_receipt_run(run, document)
}

fn git_output(root: &Path, args: &[&str]) -> Result<std::process::Output, Failure> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| Failure::config("run.source", error.to_string()))
}

fn repository_source_files(
    root: &Path,
    exclude_runtime_secrets: bool,
    include_package_lock: bool,
) -> Result<Vec<String>, Failure> {
    let output = git_output(
        root,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?;
    if !output.status.success() {
        return Err(Failure::config(
            "run.source",
            format!(
                "git ls-files in {}: {}",
                root.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    let mut files = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
        .filter_map(|item| String::from_utf8(item.to_vec()).ok())
        .filter(|relative| {
            let parts = relative.split('/').collect::<Vec<_>>();
            !Path::new(relative).is_absolute()
                && !parts.contains(&"..")
                && !parts
                    .iter()
                    .any(|part| matches!(*part, "node_modules" | "test-results"))
                && (!exclude_runtime_secrets
                    || (!parts.iter().any(|part| part.starts_with(".env"))
                        && !(relative
                            .rsplit('/')
                            .next()
                            .unwrap_or(relative)
                            .starts_with("probierz-")
                            && relative.ends_with(".json"))))
        })
        .filter(|relative| {
            fs::symlink_metadata(root.join(relative))
                .is_ok_and(|metadata| metadata.is_file() || metadata.file_type().is_symlink())
        })
        .collect::<BTreeSet<_>>();
    if include_package_lock && root.join("package-lock.json").exists() {
        files.insert("package-lock.json".into());
    }
    Ok(files.into_iter().collect())
}

fn repository_identity(
    root: &Path,
    name: &str,
    index: Option<usize>,
    exclude_runtime_secrets: bool,
    include_package_lock: bool,
) -> Result<Value, Failure> {
    let head = git_output(root, &["rev-parse", "HEAD"])?;
    let diff = git_output(root, &["diff", "--quiet", "HEAD", "--"])?;
    let others = git_output(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    let files = repository_source_files(root, exclude_runtime_secrets, include_package_lock)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 128 * 1024];
    for relative in files {
        let file = root.join(&relative);
        let metadata = fs::symlink_metadata(&file)?;
        let symlink = if metadata.file_type().is_symlink() {
            Some(
                fs::read_link(&file)?
                    .to_string_lossy()
                    .into_owned()
                    .into_bytes(),
            )
        } else {
            None
        };
        let header = json!({
            "path": relative,
            "kind": if symlink.is_some() { "symlink" } else { "file" },
            "mode": file_mode(&metadata),
            "bytes": symlink.as_ref().map_or(metadata.len(), |payload| payload.len() as u64),
        });
        let encoded = serde_json::to_string(&header)?;
        digest.update(format!("{}:", encoded.len()).as_bytes());
        digest.update(encoded.as_bytes());
        if let Some(payload) = symlink {
            digest.update(payload);
        } else {
            let mut input = File::open(&file)?;
            loop {
                let count = input.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                digest.update(&buffer[..count]);
            }
        }
    }
    let worktree = hex::encode(digest.finalize());
    let git_sha = if head.status.success() {
        Value::String(String::from_utf8_lossy(&head.stdout).trim().to_string())
    } else {
        Value::Null
    };
    let dirty = !diff.status.success() || !others.stdout.is_empty();
    let mut identity = Map::new();
    if let Some(index) = index {
        identity.insert("index".into(), json!(index));
    }
    identity.insert("name".into(), json!(name));
    identity.insert("gitSha".into(), git_sha);
    identity.insert("dirty".into(), json!(dirty));
    identity.insert("worktreeSha256".into(), json!(worktree));
    let exact = if let Some(index) = index {
        json!({ "index": index, "name": name, "worktreeSha256": worktree })
    } else {
        json!({ "name": name, "worktreeSha256": worktree })
    };
    identity.insert(
        "sha256".into(),
        json!(sha256_bytes(serde_json::to_string(&exact)?.as_bytes())),
    );
    Ok(Value::Object(identity))
}

fn app_source_identity(harness: &Path, app_id: &str) -> Result<Value, Failure> {
    if let Some(file) = std::env::var_os("PROBIERZ_SOURCE_IDENTITY") {
        let parsed = json_file(Path::new(&file))?;
        if parsed.get("schemaVersion").and_then(Value::as_i64) != Some(1) {
            return Err(Failure::config(
                "run.source",
                format!("{}: schemaVersion must be 1", Path::new(&file).display()),
            ));
        }
        if parsed
            .pointer("/harness/worktreeSha256")
            .and_then(Value::as_str)
            .is_none_or(|value| !is_sha256(value))
        {
            return Err(Failure::config(
                "run.source",
                format!(
                    "{}: harness worktreeSha256 is missing",
                    Path::new(&file).display()
                ),
            ));
        }
        for repository in parsed
            .pointer("/app/repositories")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if repository
                .get("worktreeSha256")
                .and_then(Value::as_str)
                .is_none_or(|value| !is_sha256(value))
            {
                let name = repository
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                return Err(Failure::config(
                    "run.source",
                    format!(
                        "{}: repository {name} has no worktreeSha256",
                        Path::new(&file).display()
                    ),
                ));
            }
        }
        if parsed
            .get("appId")
            .and_then(Value::as_str)
            .is_none_or(|declared| declared == app_id)
        {
            return Ok(parsed);
        }
    }
    let loaded = manifest::load(harness, app_id)?;
    let document = yaml_json(&loaded.document)?;
    let mut repositories = Vec::new();
    for (index, repository) in document
        .get("repositories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let root = PathBuf::from(
            repository
                .get("root")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        let name = root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        repositories.push(repository_identity(&root, name, Some(index), false, false)?);
    }
    let app_exact = Value::Array(repositories.iter().map(|repo| json!({
        "index": repo.get("index").cloned().unwrap_or(Value::Null), "sha256": repo.get("sha256").cloned().unwrap_or(Value::Null),
    })).collect());
    let app = json!({ "sha256": sha256_bytes(serde_json::to_string(&app_exact)?.as_bytes()), "repositories": repositories });
    Ok(json!({
        "schemaVersion": 1, "appId": app_id,
        "harness": repository_identity(harness, "probierz", None, true, true)?,
        "app": app,
    }))
}
pub(crate) fn app_source_identity_value(harness: &Path, app_id: &str) -> Result<Value, Failure> {
    app_source_identity(harness, app_id)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
fn is_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn signing_key(input: &[u8]) -> Result<SigningKey, Failure> {
    let text = String::from_utf8_lossy(input).trim().to_string();
    let key = if text.contains("BEGIN") {
        SigningKey::from_pkcs8_pem(&text)
    } else {
        let der = BASE64
            .decode(text)
            .map_err(|error| Failure::config("evidence.receipt", error.to_string()))?;
        SigningKey::from_pkcs8_der(&der)
    };
    key.map_err(|error| {
        Failure::config(
            "evidence.receipt",
            format!("evidence private key must be Ed25519: {error}"),
        )
    })
}

fn verifying_key(input: &[u8]) -> Result<VerifyingKey, Failure> {
    let text = String::from_utf8_lossy(input).trim().to_string();
    if text.contains("BEGIN") {
        VerifyingKey::from_public_key_pem(&text)
    } else {
        VerifyingKey::from_public_key_der(input)
    }
    .map_err(|error| {
        Failure::config(
            "evidence.verify_receipt",
            format!("receipt public key must be Ed25519: {error}"),
        )
    })
}

fn public_der(key: &VerifyingKey) -> Result<Vec<u8>, Failure> {
    Ok(key
        .to_public_key_der()
        .map_err(|error| Failure::config("evidence.receipt", error.to_string()))?
        .as_bytes()
        .to_vec())
}

fn sign_payload(payload: &Value, private_key: &[u8]) -> Result<Value, Failure> {
    let private = signing_key(private_key)?;
    let public = private.verifying_key();
    let canonical_payload = canonical(payload);
    let signature = private.sign(canonical_payload.as_bytes());
    let pem = public
        .to_public_key_pem(LineEnding::LF)
        .map_err(|error| Failure::config("evidence.receipt", error.to_string()))?;
    Ok(json!({
        "algorithm": "Ed25519",
        "publicKeyFingerprintSha256": sha256_bytes(&public_der(&public)?),
        "publicKeyPem": pem,
        "payloadSha256": sha256_bytes(canonical_payload.as_bytes()),
        "signature": BASE64.encode(signature.to_bytes()),
    }))
}

fn signed_evidence_id(payload: &Value, signing: &Value) -> String {
    let signature = signing
        .get("signature")
        .and_then(Value::as_str)
        .unwrap_or_default();
    sha256_bytes(format!("{}\n{signature}", canonical(payload)).as_bytes())[..24].to_string()
}

pub fn receipt(
    harness: &Path,
    app_id: Option<&str>,
    release: Option<&str>,
    expected_harness: Option<&str>,
    expected_source: Option<&str>,
    runs_csv: Option<&str>,
    journeys_csv: Option<&str>,
    minimum: &str,
) -> Answer {
    let app_id = app_id.unwrap_or_default();
    let release = release.unwrap_or_default();
    let expected_harness = expected_harness.unwrap_or_default();
    let expected_source = expected_source.unwrap_or_default();
    if app_id.is_empty()
        || release.is_empty()
        || expected_harness.is_empty()
        || expected_source.is_empty()
    {
        return Err(Failure::invalid(
            "evidence.receipt",
            "appId, release, expectedHarnessSha, and expectedSourceSha are required",
        ));
    }
    if !is_sha256(expected_harness) || !is_sha256(expected_source) {
        return Err(Failure::invalid(
            "evidence.receipt",
            "expectedHarnessSha and expectedSourceSha must be lowercase SHA-256 values",
        ));
    }
    let run_ids = runs_csv
        .unwrap_or_default()
        .split(',')
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if run_ids.is_empty() {
        return Err(Failure::invalid(
            "evidence.receipt",
            "at least one runId is required",
        ));
    }
    if !matches!(minimum, "E0" | "E1" | "E2" | "E3" | "E4" | "E5") {
        return Err(Failure::invalid(
            "evidence.receipt",
            format!("unknown evidence level: {minimum}"),
        ));
    }
    let key_file = std::env::var_os("PROBIERZ_RECEIPT_PRIVATE_KEY_FILE").ok_or_else(|| {
        Failure::config(
            "evidence.receipt",
            "PROBIERZ_RECEIPT_PRIVATE_KEY_FILE is required",
        )
    })?;
    let loaded = manifest::load(harness, app_id)?;
    let document = yaml_json(&loaded.document)?;
    let source_runs = run_ids
        .iter()
        .map(|run_id| get_run(harness, app_id, run_id))
        .collect::<Result<Vec<_>, _>>()?;
    let normalized = source_runs
        .iter()
        .map(|run| signed_receipt_run(run, &document))
        .collect::<Vec<_>>();
    let current = app_source_identity(harness, app_id)?;
    let mut errors = Vec::<String>::new();
    if current.pointer("/harness/sha256").and_then(Value::as_str) != Some(expected_harness) {
        errors.push(
            "expected harness source is stale relative to the current Probierz checkout".into(),
        );
    }
    if current.pointer("/app/sha256").and_then(Value::as_str) != Some(expected_source) {
        errors.push("expected app source is stale relative to the current product checkout".into());
    }
    let mut scans = Map::new();
    for (source, signed) in source_runs.iter().zip(&normalized) {
        let run_id = signed
            .get("runId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let artifact_root = PathBuf::from(
            source
                .get("manifestPath")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        let artifact_root = artifact_root.parent().unwrap_or(Path::new(""));
        if source
            .pointer("/protection/plaintextRemoved")
            .and_then(Value::as_bool)
            == Some(true)
        {
            let protected_root =
                absolute(&harness.join("test-results").join(".protected").join(app_id))?;
            let bundle = absolute(Path::new(
                source
                    .pointer("/protection/file")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ))?;
            if !bundle.starts_with(&protected_root) || !bundle.is_file() {
                errors.push(format!(
                    "{run_id}: protected artifact is missing or escapes its product root"
                ));
            } else if sha256_file(&bundle)?
                != signed
                    .pointer("/protection/sha256")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            {
                errors.push(format!("{run_id}: protected artifact hash mismatch"));
            }
        } else {
            for artifact in signed
                .get("artifacts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let relative = artifact
                    .get("file")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let file = absolute(&artifact_root.join(relative))?;
                let root = absolute(artifact_root)?;
                if !file.starts_with(&root) || !file.is_file() {
                    errors.push(format!(
                        "{run_id}: artifact is missing or escapes its run: {relative}"
                    ));
                } else if sha256_file(&file)?
                    != artifact
                        .get("sha256")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                {
                    errors.push(format!("{run_id}: artifact hash mismatch: {relative}"));
                }
            }
        }
        let scan = if source
            .pointer("/protection/plaintextRemoved")
            .and_then(Value::as_bool)
            == Some(true)
        {
            source.pointer("/protection/secretScan").cloned()
        } else {
            Some(scan_secrets(artifact_root)?)
        };
        let normalized_scan = scan.as_ref().map(|scan| json!({
            "passed": scan.get("passed").and_then(Value::as_bool).unwrap_or(false),
            "scannedAt": scan.get("scannedAt").cloned().unwrap_or(Value::Null),
            "scannedFiles": scan.get("scannedFiles").and_then(Value::as_f64).map(js_number).unwrap_or_else(|| json!(0)),
            "skippedBinary": scan.get("skippedBinary").and_then(Value::as_f64).map(js_number).unwrap_or_else(|| json!(0)),
            "findings": scan.get("findings").cloned().filter(Value::is_array).unwrap_or_else(|| json!([])),
        })).unwrap_or(Value::Null);
        if scan
            .as_ref()
            .and_then(|scan| scan.get("passed"))
            .and_then(Value::as_bool)
            != Some(true)
        {
            errors.push(format!(
                "{run_id}: plaintext secret scan is missing or has findings"
            ));
        }
        scans.insert(run_id.to_string(), normalized_scan);
    }
    let levels = |name: &str| match name {
        "E0" => 0,
        "E1" => 1,
        "E2" => 2,
        "E3" => 3,
        "E4" => 4,
        "E5" => 5,
        _ => -1,
    };
    for run in &normalized {
        let run_id = run.get("runId").and_then(Value::as_str).unwrap_or_default();
        let status = run
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if status != "passed" {
            errors.push(format!("{run_id}: status {status}"));
        }
        if run.pointer("/harness/sha256").and_then(Value::as_str) != Some(expected_harness)
            || run
                .pointer("/harness/gitSha")
                .and_then(Value::as_str)
                .is_none_or(|value| !is_git_sha(value))
            || run
                .pointer("/harness/worktreeSha256")
                .and_then(Value::as_str)
                .is_none_or(|value| !is_sha256(value))
        {
            errors.push(format!(
                "{run_id}: harness source identity mismatch or incomplete"
            ));
        }
        let bad_source = run.pointer("/source/sha256").and_then(Value::as_str)
            != Some(expected_source)
            || run
                .pointer("/source/repositories")
                .and_then(Value::as_array)
                .is_none_or(|repositories| {
                    repositories.iter().any(|repository| {
                        repository
                            .get("gitSha")
                            .and_then(Value::as_str)
                            .is_none_or(|value| !is_git_sha(value))
                            || repository
                                .get("worktreeSha256")
                                .and_then(Value::as_str)
                                .is_none_or(|value| !is_sha256(value))
                    })
                });
        if bad_source {
            errors.push(format!(
                "{run_id}: app source identity mismatch or incomplete"
            ));
        }
        if run
            .pointer("/build/sha256")
            .and_then(Value::as_str)
            .is_none_or(|value| !is_sha256(value))
        {
            errors.push(format!("{run_id}: build hash missing or invalid"));
        }
        if run
            .get("artifacts")
            .and_then(Value::as_array)
            .is_none_or(|artifacts| {
                artifacts.is_empty()
                    || artifacts.iter().any(|artifact| {
                        artifact
                            .get("sha256")
                            .and_then(Value::as_str)
                            .is_none_or(|value| !is_sha256(value))
                    })
            })
        {
            errors.push(format!("{run_id}: artifact hashes incomplete or invalid"));
        }
        let level = run
            .get("evidenceLevel")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if levels(level) < levels(minimum) {
            errors.push(format!("{run_id}: {level} is below {minimum}"));
        }
    }
    let mut builds = Map::new();
    for run in &normalized {
        let Some(hash) = run.pointer("/build/sha256").and_then(Value::as_str) else {
            continue;
        };
        let target = run
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if builds
            .get(target)
            .and_then(Value::as_str)
            .is_some_and(|old| old != hash)
        {
            errors.push(format!("{target}: runs do not identify one exact build"));
        } else {
            builds.insert(target.to_string(), json!(hash));
        }
    }
    let covered: BTreeSet<String> = normalized
        .iter()
        .filter(|run| run.get("status").and_then(Value::as_str) == Some("passed"))
        .flat_map(|run| {
            run.get("journeys")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let required: BTreeSet<String> = journeys_csv
        .unwrap_or_default()
        .split(',')
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    let missing: Vec<String> = required.difference(&covered).cloned().collect();
    errors.extend(
        missing
            .iter()
            .map(|journey| format!("missing journey: {journey}")),
    );
    let mut redact_policy = document
        .pointer("/artifacts/redact")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    redact_policy.sort_by(|a, b| {
        a.as_str()
            .unwrap_or_default()
            .cmp(b.as_str().unwrap_or_default())
    });
    let payload = json!({
        "schemaVersion": 3, "kind": "probierz-evidence-receipt", "appId": app_id, "release": release,
        "expectedHarnessSha": expected_harness, "expectedSourceSha": expected_source, "builds": builds,
        "productId": document.get("productId").cloned().unwrap_or_else(|| json!(app_id)),
        "artifactPolicy": {
            "retain": document.pointer("/artifacts/retain").cloned().unwrap_or_else(|| json!({})),
            "redact": redact_policy,
            "pii": document.pointer("/artifacts/pii").cloned().unwrap_or_else(|| json!("unknown")),
        },
        "secretScans": scans, "issuedAt": now_iso(),
        "policy": { "minimumEvidence": minimum, "requiredJourneys": required },
        "verdict": { "passed": errors.is_empty(), "errors": errors, "coveredJourneys": covered, "missingJourneys": missing },
        "runs": normalized,
    });
    let signing = sign_payload(&payload, &fs::read(key_file)?)?;
    let mut receipt_value = payload.as_object().cloned().unwrap_or_default();
    receipt_value.insert("signing".into(), signing.clone());
    let receipt_value = Value::Object(receipt_value);
    let receipt_id = signed_evidence_id(&payload, &signing);
    let file = harness
        .join("test-results")
        .join("receipts")
        .join(segment(app_id, "unknown"))
        .join(segment(release, "unknown"))
        .join(format!("{receipt_id}.json"));
    write_new_json(&file, &receipt_value, true)?;
    let result = json!({ "file": file.to_string_lossy(), "receiptId": receipt_id, "receipt": receipt_value });
    print_json(&result)?;
    if result
        .pointer("/receipt/verdict/passed")
        .and_then(Value::as_bool)
        != Some(true)
    {
        std::process::exit(1);
    }
    Ok(())
}

pub fn verify_receipt_value(
    file: &Path,
    trusted_public_key: Option<&Path>,
    expected_fingerprint: Option<&str>,
) -> Result<Value, Failure> {
    let receipt_value = json_file(file)?;
    let mut payload_map = receipt_value
        .as_object()
        .cloned()
        .ok_or_else(|| Failure::invalid("evidence.verify_receipt", "receipt is not an object"))?;
    let signing = payload_map.remove("signing").ok_or_else(|| {
        Failure::invalid(
            "evidence.verify_receipt",
            "unsupported or missing receipt signature",
        )
    })?;
    if signing.get("algorithm").and_then(Value::as_str) != Some("Ed25519") {
        return Err(Failure::invalid(
            "evidence.verify_receipt",
            "unsupported or missing receipt signature",
        ));
    }
    let public = match trusted_public_key {
        Some(path) => verifying_key(&fs::read(path)?)?,
        None => verifying_key(
            signing
                .get("publicKeyPem")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .as_bytes(),
        )?,
    };
    let fingerprint = sha256_bytes(&public_der(&public)?);
    let expected = expected_fingerprint
        .map(str::to_string)
        .or_else(|| std::env::var("PROBIERZ_RECEIPT_PUBLIC_KEY_FINGERPRINT").ok());
    let canonical_payload = canonical(&Value::Object(payload_map.clone()));
    let signature_valid = BASE64
        .decode(
            signing
                .get("signature")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .is_some_and(|signature| {
            public
                .verify(canonical_payload.as_bytes(), &signature)
                .is_ok()
        });
    let trusted = trusted_public_key.is_some()
        || expected
            .as_deref()
            .is_some_and(|value| value == fingerprint);
    let payload_hash = sha256_bytes(canonical_payload.as_bytes());
    let valid = signature_valid
        && trusted
        && signing.get("payloadSha256").and_then(Value::as_str) == Some(&payload_hash);
    let payload = Value::Object(payload_map);
    let mut answer = Map::new();
    answer.insert("valid".into(), json!(valid));
    answer.insert("signatureValid".into(), json!(signature_valid));
    answer.insert("trusted".into(), json!(trusted));
    answer.insert("fingerprint".into(), json!(fingerprint));
    answer.insert("payloadSha256".into(), json!(payload_hash));
    answer.insert(
        "receiptId".into(),
        json!(signed_evidence_id(&payload, &signing)),
    );
    for key in [
        "issuedAt",
        "productId",
        "verdict",
        "appId",
        "release",
        "expectedHarnessSha",
        "expectedSourceSha",
        "builds",
    ] {
        if key == "productId" {
            if let Some(value) = payload.get("productId").or_else(|| payload.get("appId")) {
                answer.insert(key.into(), value.clone());
            }
        } else if let Some(value) = payload.get(key) {
            answer.insert(key.into(), value.clone());
        }
    }
    answer.insert(
        "secretScans".into(),
        payload
            .get("secretScans")
            .cloned()
            .unwrap_or_else(|| json!({})),
    );
    if let Some(value) = payload.get("policy") {
        answer.insert("policy".into(), value.clone());
    }
    let runs = payload.get("runs").cloned().unwrap_or_else(|| json!([]));
    answer.insert("runs".into(), runs.clone());
    answer.insert(
        "runIds".into(),
        Value::Array(
            runs.as_array()
                .into_iter()
                .flatten()
                .map(|run| run.get("runId").cloned().unwrap_or(Value::Null))
                .collect(),
        ),
    );
    Ok(Value::Object(answer))
}

pub fn verify_receipt(
    file: Option<&Path>,
    public_key: Option<&Path>,
    fingerprint: Option<&str>,
) -> Answer {
    let file = file.ok_or_else(|| {
        Failure::invalid("evidence.verify_receipt", "verify-receipt needs a file")
    })?;
    let result = verify_receipt_value(file, public_key, fingerprint)?;
    print_json(&result)?;
    if result.get("valid").and_then(Value::as_bool) != Some(true) {
        std::process::exit(1);
    }
    Ok(())
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), Failure> {
    if condition {
        Ok(())
    } else {
        Err(Failure::invalid(
            "evidence.publication",
            format!("publication rejected: {}", message.into()),
        ))
    }
}

fn require_string<'a>(value: Option<&'a Value>, message: &str) -> Result<&'a str, Failure> {
    let text = value.and_then(Value::as_str).unwrap_or_default();
    require(!text.is_empty(), message)?;
    Ok(text)
}

fn parse_iso(value: &str, name: &str) -> Result<DateTime<Utc>, Failure> {
    DateTime::parse_from_rfc3339(value)
        .map(|at| at.with_timezone(&Utc))
        .map_err(|_| {
            Failure::invalid(
                "evidence.publication",
                format!("publication rejected: {name} must be an ISO timestamp"),
            )
        })
}

fn immutable_storage_url(value: &str, require_path: bool) -> bool {
    Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.host_str().is_some()
            && (!require_path || url.path() != "/")
    })
}

fn source_revision(source: Option<&Value>) -> Option<&str> {
    let repositories = source?.get("repositories")?.as_array()?;
    repositories
        .iter()
        .find(|entry| entry.get("index").and_then(Value::as_i64) == Some(0))
        .or_else(|| repositories.first())?
        .get("gitSha")?
        .as_str()
}

pub fn publication(
    harness: &Path,
    receipt_file: Option<&Path>,
    attempt_id: Option<&str>,
    journey_id: Option<&str>,
    assets_file: Option<&Path>,
    public_key: Option<&Path>,
    fingerprint: Option<&str>,
) -> Answer {
    let (receipt_file, attempt_id, journey_id, assets_file) =
        match (receipt_file, attempt_id, journey_id, assets_file) {
            (Some(receipt), Some(attempt), Some(journey), Some(assets)) => {
                (receipt, attempt, journey, assets)
            }
            _ => {
                return Err(Failure::invalid(
                    "evidence.publication",
                    "publication needs receipt, attemptId, journeyId, and --assets <json>",
                ))
            }
        };
    let assets = json_file(assets_file)?;
    let result = create_publication(
        harness,
        receipt_file,
        attempt_id,
        journey_id,
        &assets,
        public_key,
        fingerprint,
    )?;
    print_json(&result)
}

fn create_publication(
    harness: &Path,
    receipt_file: &Path,
    attempt_id: &str,
    journey_id: &str,
    assets: &Value,
    public_key: Option<&Path>,
    fingerprint: Option<&str>,
) -> Result<Value, Failure> {
    require(!attempt_id.is_empty(), "attemptId is required")?;
    require(!journey_id.is_empty(), "journeyId is required")?;
    require(
        assets.as_array().is_some_and(|values| !values.is_empty()),
        "at least one asset registration is required",
    )?;
    let verification = verify_receipt_value(receipt_file, public_key, fingerprint)?;
    require(
        verification.get("valid").and_then(Value::as_bool) == Some(true)
            && verification.get("signatureValid").and_then(Value::as_bool) == Some(true)
            && verification.get("trusted").and_then(Value::as_bool) == Some(true),
        "receipt signature is not valid and trusted",
    )?;
    require(
        verification
            .pointer("/verdict/passed")
            .and_then(Value::as_bool)
            == Some(true),
        "receipt verdict did not pass",
    )?;
    let product = require_string(
        verification.get("productId"),
        "receipt productId is missing",
    )?;
    let expected_source = require_string(
        verification.get("expectedSourceSha"),
        "receipt source SHA-256 is missing",
    )?;
    let signed_receipt = json_file(receipt_file)?;
    require(
        signed_receipt
            .get("schemaVersion")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            >= 3,
        "receipt predates publication provenance",
    )?;
    let run = verification
        .get("runs")
        .and_then(Value::as_array)
        .and_then(|runs| {
            runs.iter()
                .find(|run| run.get("runId").and_then(Value::as_str) == Some(attempt_id))
        })
        .ok_or_else(|| {
            Failure::invalid(
                "evidence.publication",
                format!("publication rejected: attempt {attempt_id} is not signed by the receipt"),
            )
        })?;
    require(
        run.get("status").and_then(Value::as_str) == Some("passed"),
        format!("attempt {attempt_id} did not pass"),
    )?;
    require(
        run.get("media")
            .and_then(Value::as_array)
            .is_some_and(|media| !media.is_empty()),
        format!("attempt {attempt_id} has no signed evidence artifacts"),
    )?;
    let app = manifest::load(
        harness,
        verification
            .get("appId")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )?;
    let document = yaml_json(&app.document)?;
    require(
        document.get("productId").and_then(Value::as_str) == Some(product),
        "app manifest and receipt productId differ",
    )?;
    let identity = run.get("journeyIdentities").and_then(Value::as_array).and_then(|values| values.iter().find(|value| value.get("journeyId").and_then(Value::as_str) == Some(journey_id)))
        .ok_or_else(|| Failure::invalid("evidence.publication", format!("publication rejected: journey {journey_id} is not signed for attempt {attempt_id}")))?;
    let configured = document
        .get("journeys")
        .and_then(Value::as_object)
        .and_then(|journeys| {
            journeys
                .values()
                .find(|journey| journey.get("journeyVersionId") == identity.get("journeyVersionId"))
        })
        .ok_or_else(|| {
            Failure::invalid(
                "evidence.publication",
                "publication rejected: journey version is no longer present in the app manifest",
            )
        })?;
    require(
        configured.get("journeyId") == identity.get("journeyId")
            && configured.get("journeyVersion") == identity.get("journeyVersion")
            && configured.get("firstSuccessFact") == identity.get("firstSuccessFact"),
        "journey identity changed after receipt issuance",
    )?;
    let policy = configured.get("publication").ok_or_else(|| {
        Failure::invalid(
            "evidence.publication",
            "publication rejected: journey has no publication policy",
        )
    })?;
    let current = app_source_identity(
        harness,
        verification
            .get("appId")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )?;
    require(
        current.pointer("/app/sha256").and_then(Value::as_str) == Some(expected_source),
        "receipt source is stale relative to the current product source",
    )?;
    require(
        run.pointer("/source/sha256").and_then(Value::as_str) == Some(expected_source),
        "attempt source does not match the receipt source",
    )?;
    let revision = source_revision(run.get("source"));
    require(
        revision.is_some_and(is_git_sha),
        "primary source revision must be a full Git SHA-40",
    )?;
    require(
        canonical(policy) == canonical(identity.get("publication").unwrap_or(&Value::Null)),
        "publication policy changed after receipt issuance",
    )?;
    require(
        source_revision(current.get("app")) == revision,
        "primary source revision is stale",
    )?;
    require(
        run.pointer("/build/sha256")
            .and_then(Value::as_str)
            .is_some_and(is_sha256),
        "attempt build SHA-256 is missing",
    )?;
    require(
        verification
            .get("release")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty()),
        "receipt release version is missing",
    )?;
    let scan = verification.pointer(&format!(
        "/secretScans/{}",
        attempt_id.replace('~', "~0").replace('/', "~1")
    ));
    require(
        scan.and_then(|scan| scan.get("passed"))
            .and_then(Value::as_bool)
            == Some(true)
            && scan
                .and_then(|scan| scan.get("findings"))
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty),
        "plaintext secret scan is missing or has findings",
    )?;
    let evidence = run
        .get("evidenceLevel")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let score = |value: &str| match value {
        "E0" => Some(0),
        "E1" => Some(1),
        "E2" => Some(2),
        "E3" => Some(3),
        _ => None,
    };
    require(
        score(evidence).is_some(),
        format!("unsupported evidence level {evidence}"),
    )?;
    let minimum = policy
        .get("minimumEvidence")
        .and_then(Value::as_str)
        .unwrap_or_default();
    require(
        score(evidence).unwrap_or(-1) >= score(minimum).unwrap_or(99),
        format!("{evidence} is below {minimum}"),
    )?;
    let signed_media: HashMap<&str, &Value> = run
        .get("media")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|media| {
            media
                .get("file")
                .and_then(Value::as_str)
                .map(|file| (file, media))
        })
        .collect();
    let mut seen = HashSet::new();
    let mut publication_assets = Vec::new();
    for (index, registration) in assets.as_array().unwrap().iter().enumerate() {
        require(
            registration.is_object(),
            format!("assets.{index} must be an object"),
        )?;
        let file = require_string(
            registration.get("file"),
            &format!("assets.{index}.file is required"),
        )?;
        require(
            seen.insert(file),
            format!("assets.{index}.file is duplicated"),
        )?;
        let media = signed_media.get(file).copied().ok_or_else(|| {
            Failure::invalid(
                "evidence.publication",
                format!(
                    "publication rejected: assets.{index}.file is not signed report-typed evidence"
                ),
            )
        })?;
        let kind = media
            .get("artifactKind")
            .and_then(Value::as_str)
            .unwrap_or_default();
        require(
            matches!(kind, "screenshot" | "recording" | "trace"),
            format!("assets.{index} has unsupported artifact kind {kind}"),
        )?;
        require(
            policy
                .get("artifactKinds")
                .and_then(Value::as_array)
                .is_some_and(|kinds| kinds.iter().any(|value| value.as_str() == Some(kind))),
            format!("assets.{index} kind {kind} is not allowed by the journey policy"),
        )?;
        require(
            manifest::target_supports_artifact_kind(
                run.get("target")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                kind,
            ),
            format!(
                "driver {} does not support {kind}",
                run.get("target")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            ),
        )?;
        require(
            registration
                .get("kind")
                .is_none_or(|value| value.as_str() == Some(kind)),
            format!("assets.{index}.kind conflicts with signed evidence"),
        )?;
        let content = registration
            .get("contentSha256")
            .and_then(Value::as_str)
            .unwrap_or_default();
        require(
            is_sha256(content),
            format!("assets.{index}.contentSha256 is required"),
        )?;
        require(
            media.get("contentSha256").and_then(Value::as_str) == Some(content),
            format!("assets.{index}.contentSha256 does not match the signed receipt"),
        )?;
        let storage = registration
            .get("storageUrl")
            .and_then(Value::as_str)
            .unwrap_or_default();
        require(immutable_storage_url(storage, true), format!("assets.{index}.storageUrl must be immutable HTTPS without credentials, query, or fragment"))?;
        let redaction = registration
            .get("redactionStatus")
            .and_then(Value::as_str)
            .unwrap_or_default();
        require(
            matches!(redaction, "verified_redacted" | "not_applicable"),
            format!("assets.{index}.redactionStatus is unsupported"),
        )?;
        if policy.get("redactionRequired").and_then(Value::as_bool) == Some(true) {
            require(
                redaction == "verified_redacted",
                format!("assets.{index} lacks required redaction verification"),
            )?;
        }
        let verified_text = registration
            .get("verifiedAt")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let verified = parse_iso(verified_text, &format!("assets.{index}.verifiedAt"))?;
        let captured_text = media
            .get("capturedAt")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let captured = parse_iso(captured_text, &format!("assets.{index}.capturedAt"))?;
        require(
            verified >= captured,
            format!("assets.{index}.verifiedAt predates capture"),
        )?;
        let asset = json!({
            "attemptId": attempt_id, "screenId": policy.get("screenId").cloned().unwrap_or(Value::Null), "kind": kind,
            "storageUrl": storage, "contentSha256": content,
            "bytes": media.get("bytes").and_then(Value::as_f64).map(js_number).unwrap_or_else(|| json!(0)),
            "evidenceLevel": evidence, "redactionStatus": redaction, "capturedAt": captured_text, "verifiedAt": verified_text,
        });
        let mut with_id = Map::new();
        with_id.insert(
            "artifactId".into(),
            json!(sha256_bytes(canonical(&asset).as_bytes())),
        );
        with_id.extend(asset.as_object().cloned().unwrap_or_default());
        publication_assets.push(Value::Object(with_id));
    }
    publication_assets.sort_by(|left, right| {
        left.get("artifactId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(
                right
                    .get("artifactId")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
    });
    let verified_at = publication_assets
        .iter()
        .filter_map(|asset| asset.get("verifiedAt").and_then(Value::as_str))
        .max()
        .unwrap_or_default();
    let captured_at = run
        .get("completedAt")
        .or_else(|| run.get("startedAt"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    parse_iso(captured_at, "attempt.capturedAt")?;
    let unsigned = json!({
        "schemaVersion": 1, "kind": "probierz-first-use-publication", "publishable": true, "productId": product,
        "journey": {
            "journeyId": identity.get("journeyId").cloned().unwrap_or(Value::Null), "journeyVersion": identity.get("journeyVersion").cloned().unwrap_or(Value::Null),
            "journeyVersionId": identity.get("journeyVersionId").cloned().unwrap_or(Value::Null), "firstSuccessFact": identity.get("firstSuccessFact").cloned().unwrap_or(Value::Null),
            "screenId": policy.get("screenId").cloned().unwrap_or(Value::Null),
        },
        "release": { "version": verification.get("release").cloned().unwrap_or(Value::Null), "sourceRevision": revision, "sourceSha256": expected_source, "buildSha256": run.pointer("/build/sha256").cloned().unwrap_or(Value::Null) },
        "attempt": { "attemptId": attempt_id, "evidenceLevel": evidence, "capturedAt": captured_at, "verifiedAt": verified_at },
        "receipt": {
            "receiptId": verification.get("receiptId").cloned().unwrap_or(Value::Null), "signed": signed_receipt,
            "verification": { "valid": true, "signatureValid": true, "trusted": true,
                "fingerprint": verification.get("fingerprint").cloned().unwrap_or(Value::Null), "payloadSha256": verification.get("payloadSha256").cloned().unwrap_or(Value::Null),
                "verifiedAt": verification.get("issuedAt").cloned().unwrap_or(Value::Null) },
        },
        "assets": publication_assets,
    });
    let mut publication = unsigned.as_object().cloned().unwrap_or_default();
    publication.insert(
        "manifestId".into(),
        json!(sha256_bytes(canonical(&unsigned).as_bytes())),
    );
    // JS adds manifestId last for this command.
    let publication = Value::Object(publication);
    let manifest_id = publication
        .get("manifestId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let file = harness
        .join("test-results")
        .join("publications")
        .join(segment(product, "unknown"))
        .join(segment(
            verification
                .get("release")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "unknown",
        ))
        .join(segment(
            identity
                .get("journeyVersionId")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "unknown",
        ))
        .join(segment(attempt_id, "unknown"))
        .join(format!("{manifest_id}.json"));
    let serialized = format!("{}\n", serde_json::to_string_pretty(&publication)?);
    let reused = if file.exists() {
        require(
            fs::read_to_string(&file)? == serialized,
            "immutable publication manifest path contains different content",
        )?;
        true
    } else {
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)?;
        output.write_all(serialized.as_bytes())?;
        drop(output);
        apply_mode(&file, 0o600)?;
        false
    };
    Ok(
        json!({ "file": file.to_string_lossy(), "manifestId": manifest_id, "publication": publication, "reused": reused }),
    )
}

fn required_raw<'a>(
    value: Option<&'a str>,
    name: &str,
    predicate: impl Fn(&str) -> bool,
) -> Result<&'a str, Failure> {
    let value = value.unwrap_or_default();
    if value.is_empty() || !predicate(value) {
        Err(Failure::invalid(
            "evidence.onboarding_publication",
            format!("{name} is invalid"),
        ))
    } else {
        Ok(value)
    }
}

fn identifier(value: &str) -> bool {
    value.len() <= 128
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
fn uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
        && matches!(bytes[14], b'1'..=b'5')
        && matches!(bytes[19].to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b')
}

#[allow(clippy::too_many_arguments)]
pub fn publish_onboarding(
    receipt_file: Option<&Path>,
    run_id: Option<&str>,
    journey_id: Option<&str>,
    journey_version: Option<&str>,
    journey_version_id: Option<&str>,
    first_success_fact: Option<&str>,
    screen_id: Option<&str>,
    asset_catalog: Option<&Path>,
    output_file: Option<&Path>,
    public_key: Option<&Path>,
    fingerprint: Option<&str>,
) -> Answer {
    let receipt_file = receipt_file.ok_or_else(|| {
        Failure::invalid("evidence.onboarding_publication", "receipt file is invalid")
    })?;
    let run_id = required_raw(run_id, "run id", |_| true)?;
    let journey_id = required_raw(journey_id, "journey id", identifier)?;
    let journey_version = required_raw(journey_version, "journey version", |_| true)?;
    let journey_version_id = required_raw(journey_version_id, "journey version id", uuid)?;
    let first_success_fact = required_raw(first_success_fact, "first success fact", identifier)?;
    let screen_id = required_raw(screen_id, "screen id", identifier)?;
    let asset_catalog = asset_catalog.ok_or_else(|| {
        Failure::invalid(
            "evidence.onboarding_publication",
            "asset catalog file is invalid",
        )
    })?;
    let result = create_onboarding_publication(
        receipt_file,
        run_id,
        journey_id,
        journey_version,
        journey_version_id,
        first_success_fact,
        screen_id,
        asset_catalog,
        output_file,
        public_key,
        fingerprint,
    )?;
    print_json(
        &json!({ "file": result.get("file").cloned().unwrap_or(Value::Null), "manifestId": result.get("manifestId").cloned().unwrap_or(Value::Null) }),
    )
}

#[allow(clippy::too_many_arguments)]
fn create_onboarding_publication(
    receipt_file: &Path,
    run_id: &str,
    journey_id: &str,
    journey_version: &str,
    journey_version_id: &str,
    first_success_fact: &str,
    screen_id: &str,
    catalog_file: &Path,
    output_file: Option<&Path>,
    public_key: Option<&Path>,
    fingerprint: Option<&str>,
) -> Result<Value, Failure> {
    let verification = verify_receipt_value(receipt_file, public_key, fingerprint)?;
    if verification.get("valid").and_then(Value::as_bool) != Some(true)
        || verification.get("signatureValid").and_then(Value::as_bool) != Some(true)
        || verification.get("trusted").and_then(Value::as_bool) != Some(true)
        || verification
            .pointer("/verdict/passed")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err(Failure::invalid(
            "evidence.onboarding_publication",
            "receipt is not valid, trusted, and passing",
        ));
    }
    let receipt = json_file(receipt_file)?;
    let run = receipt
        .get("runs")
        .and_then(Value::as_array)
        .and_then(|runs| {
            runs.iter()
                .find(|run| run.get("runId").and_then(Value::as_str) == Some(run_id))
        })
        .ok_or_else(|| {
            Failure::invalid(
                "evidence.onboarding_publication",
                format!("passing receipt run not found: {run_id}"),
            )
        })?;
    if run.get("status").and_then(Value::as_str) != Some("passed") {
        return Err(Failure::invalid(
            "evidence.onboarding_publication",
            format!("passing receipt run not found: {run_id}"),
        ));
    }
    if run
        .get("journeys")
        .and_then(Value::as_array)
        .is_none_or(|journeys| {
            !journeys
                .iter()
                .any(|value| value.as_str() == Some(journey_id))
        })
    {
        return Err(Failure::invalid(
            "evidence.onboarding_publication",
            format!("receipt run does not cover journey: {journey_id}"),
        ));
    }
    let evidence = run
        .get("evidenceLevel")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(evidence, "E2" | "E3") {
        return Err(Failure::invalid(
            "evidence.onboarding_publication",
            "onboarding publication requires E2 or E3 evidence",
        ));
    }
    if run
        .pointer("/protection/secretScan/passed")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err(Failure::invalid(
            "evidence.onboarding_publication",
            "receipt run has no successful protected-artifact secret scan",
        ));
    }
    let build = required_raw(
        run.pointer("/build/sha256").and_then(Value::as_str),
        "build sha256",
        is_sha256,
    )?;
    if run.pointer("/source/sha256").and_then(Value::as_str)
        != receipt.get("expectedSourceSha").and_then(Value::as_str)
        || run
            .pointer("/source/repositories")
            .and_then(Value::as_array)
            .is_none()
    {
        return Err(Failure::invalid(
            "evidence.onboarding_publication",
            "receipt run source identity does not match the signed receipt",
        ));
    }
    let revision = required_raw(
        source_revision(run.get("source")),
        "source revision",
        is_git_sha,
    )?;
    let catalog = json_file(catalog_file)?;
    if catalog.as_array().is_none_or(Vec::is_empty) {
        return Err(Failure::invalid(
            "evidence.onboarding_publication",
            "asset catalog must be a non-empty JSON array",
        ));
    }
    let verified_at = now_iso();
    let completed = run
        .get("completedAt")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let default_captured = DateTime::parse_from_rfc3339(completed)
        .map_err(|_| {
            Failure::invalid(
                "evidence.onboarding_publication",
                "run completedAt is invalid",
            )
        })?
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let signed_artifacts: HashMap<&str, &Value> = run
        .get("artifacts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artifact| {
            artifact
                .get("file")
                .and_then(Value::as_str)
                .map(|file| (file, artifact))
        })
        .collect();
    let mut assets = Vec::new();
    for (index, entry) in catalog.as_array().unwrap().iter().enumerate() {
        let file = required_raw(
            entry.get("file").and_then(Value::as_str),
            &format!("assets[{index}].file"),
            |_| true,
        )?;
        let signed = signed_artifacts.get(file).copied();
        if signed.is_none_or(|signed| {
            signed
                .get("sha256")
                .and_then(Value::as_str)
                .is_none_or(|value| !is_sha256(value))
                || signed
                    .get("bytes")
                    .and_then(Value::as_u64)
                    .is_none_or(|bytes| bytes == 0)
        }) {
            return Err(Failure::invalid(
                "evidence.onboarding_publication",
                format!("assets[{index}] is not bound by the signed receipt"),
            ));
        }
        let signed = signed.unwrap();
        let kind = required_raw(
            entry.get("kind").and_then(Value::as_str),
            &format!("assets[{index}].kind"),
            |_| true,
        )?;
        let redaction = required_raw(
            entry.get("redactionStatus").and_then(Value::as_str),
            &format!("assets[{index}].redactionStatus"),
            |_| true,
        )?;
        if !matches!(kind, "screenshot" | "recording" | "trace") {
            return Err(Failure::invalid(
                "evidence.onboarding_publication",
                format!("assets[{index}].kind is unsupported"),
            ));
        }
        if !matches!(redaction, "verified_redacted" | "not_applicable") {
            return Err(Failure::invalid(
                "evidence.onboarding_publication",
                format!("assets[{index}].redactionStatus is incomplete"),
            ));
        }
        let storage = required_raw(
            entry.get("storageUrl").and_then(Value::as_str),
            &format!("assets[{index}].storageUrl"),
            |_| true,
        )?;
        if !immutable_storage_url(storage, false) {
            return Err(Failure::invalid(
                "evidence.onboarding_publication",
                format!("assets[{index}].storageUrl must be a credential-free immutable HTTPS URL"),
            ));
        }
        let storage = Url::parse(storage)
            .map_err(|error| {
                Failure::invalid("evidence.onboarding_publication", error.to_string())
            })?
            .to_string();
        let captured = match entry.get("capturedAt").and_then(Value::as_str) {
            Some(value) => DateTime::parse_from_rfc3339(value)
                .map_err(|_| {
                    Failure::invalid(
                        "evidence.onboarding_publication",
                        format!("assets[{index}].capturedAt is invalid"),
                    )
                })?
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true),
            None => default_captured.clone(),
        };
        let identity = json!({
            "attemptId": run_id, "screenId": screen_id, "kind": kind, "storageUrl": storage,
            "contentSha256": signed.get("sha256").cloned().unwrap_or(Value::Null), "bytes": signed.get("bytes").cloned().unwrap_or(Value::Null),
            "evidenceLevel": evidence, "redactionStatus": redaction, "capturedAt": captured, "verifiedAt": verified_at,
        });
        let mut with_id = Map::new();
        with_id.insert(
            "artifactId".into(),
            json!(sha256_bytes(canonical(&identity).as_bytes())),
        );
        with_id.extend(identity.as_object().cloned().unwrap_or_default());
        assets.push(Value::Object(with_id));
    }
    let unique: HashSet<&str> = assets
        .iter()
        .filter_map(|asset| asset.get("contentSha256").and_then(Value::as_str))
        .collect();
    if unique.len() != assets.len() {
        return Err(Failure::invalid(
            "evidence.onboarding_publication",
            "asset catalog contains duplicate signed artifacts",
        ));
    }
    let mut signed_payload = receipt.as_object().cloned().unwrap_or_default();
    let signing = signed_payload.remove("signing").unwrap_or(Value::Null);
    let receipt_id = signed_evidence_id(&Value::Object(signed_payload), &signing);
    let identity = json!({
        "schemaVersion": 1, "kind": "probierz-first-use-publication", "publishable": true,
        "productId": required_raw(receipt.get("appId").and_then(Value::as_str), "product id", identifier)?,
        "journey": { "journeyId": journey_id, "journeyVersion": journey_version, "journeyVersionId": journey_version_id, "firstSuccessFact": first_success_fact, "screenId": screen_id },
        "release": { "version": required_raw(receipt.get("release").and_then(Value::as_str), "release version", |_| true)?, "sourceRevision": revision,
            "sourceSha256": required_raw(receipt.get("expectedSourceSha").and_then(Value::as_str), "source sha256", is_sha256)?, "buildSha256": build },
        "attempt": { "attemptId": run_id, "evidenceLevel": evidence, "capturedAt": default_captured, "verifiedAt": verified_at },
        "receipt": { "receiptId": receipt_id, "signed": receipt,
            "verification": { "valid": true, "signatureValid": true, "trusted": true,
                "fingerprint": verification.get("fingerprint").cloned().unwrap_or(Value::Null), "payloadSha256": verification.get("payloadSha256").cloned().unwrap_or(Value::Null), "verifiedAt": verified_at } },
        "assets": assets,
    });
    let mut publication = Map::new();
    publication.insert(
        "manifestId".into(),
        json!(sha256_bytes(canonical(&identity).as_bytes())),
    );
    publication.extend(identity.as_object().cloned().unwrap_or_default());
    let publication = Value::Object(publication);
    let target = match output_file {
        Some(path) => absolute(path)?,
        None => absolute(Path::new(&format!(
            "onboarding-publication-{}.json",
            publication
                .get("manifestId")
                .and_then(Value::as_str)
                .unwrap_or_default()
        )))?,
    };
    write_new_json(&target, &publication, true)?;
    Ok(
        json!({ "file": target.to_string_lossy(), "manifestId": publication.get("manifestId").cloned().unwrap_or(Value::Null), "publication": publication }),
    )
}

// Shared device/resource locks used by run.rs.
pub fn resources_for(target: &str, env: &BTreeMap<String, String>) -> Vec<String> {
    let value = |name: &str| {
        env.get(name)
            .filter(|value| !value.is_empty())
            .map(String::as_str)
    };
    let mut resources = match target {
        "mobile:ios" | "mobile:ios:byk-auth" => vec![
            format!(
                "device:ios:{}:{}",
                value("IOS_DEVICE").unwrap_or("default"),
                value("IOS_VERSION").unwrap_or("default"),
            ),
            "port:4723".into(),
        ],
        "mobile:android" => vec![
            format!(
                "device:android:{}:{}",
                value("ANDROID_DEVICE").unwrap_or("default"),
                value("ANDROID_VERSION").unwrap_or("default"),
            ),
            "port:4723".into(),
        ],
        "desktop:mac" => vec![
            format!(
                "device:mac:{}",
                value("MAC_BUNDLE_ID")
                    .or_else(|| value("MAC_APP_PATH"))
                    .unwrap_or("host"),
            ),
            "port:4723".into(),
        ],
        "desktop:win" => vec![
            format!("device:win:{}", value("WIN_APP").unwrap_or("host")),
            "port:4723".into(),
        ],
        _ => Vec::new(),
    };
    resources.sort();
    resources.dedup();
    resources
}

fn lock_name(resource: &str) -> String {
    let mut label = segment(resource, "");
    label.truncate(80);
    format!("{label}-{}", &sha256_bytes(resource.as_bytes())[..12])
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
    #[cfg(not(unix))]
    {
        pid == std::process::id()
    }
}

fn acquire_one(harness: &Path, resource: &str, owner: &str) -> Result<(PathBuf, Value), Failure> {
    let lock_root = harness.join("test-results").join(".locks");
    fs::create_dir_all(&lock_root)?;
    let directory = lock_root.join(lock_name(resource));
    for attempt in 0..2 {
        match fs::create_dir(&directory) {
            Ok(()) => {
                let owner_value = json!({ "schemaVersion": 1, "resource": resource, "runId": owner, "pid": std::process::id(), "acquiredAt": now_iso() });
                if let Err(error) =
                    write_new_json(&directory.join("owner.json"), &owner_value, true)
                {
                    let _ = fs::remove_dir_all(&directory);
                    return Err(error);
                }
                return Ok((directory, owner_value));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let current = try_json_file(&directory.join("owner.json"));
                let stale = match current.as_ref() {
                    Some(owner) => {
                        !process_alive(owner.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32)
                    }
                    None => fs::metadata(&directory)
                        .and_then(|metadata| metadata.modified())
                        .ok()
                        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                        .is_none_or(|age| age >= Duration::from_secs(30)),
                };
                if attempt == 0 && stale {
                    let tombstone = PathBuf::from(format!(
                        "{}.stale-{}-{}",
                        directory.display(),
                        std::process::id(),
                        Utc::now().timestamp_millis()
                    ));
                    if fs::rename(&directory, &tombstone).is_ok() {
                        let _ = fs::remove_dir_all(tombstone);
                        continue;
                    }
                }
                let detail = current
                    .as_ref()
                    .map(|value| {
                        format!(
                            "run {} (pid {})",
                            value
                                .get("runId")
                                .and_then(Value::as_str)
                                .unwrap_or("undefined"),
                            value.get("pid").and_then(Value::as_u64).unwrap_or(0)
                        )
                    })
                    .unwrap_or_else(|| "an unknown owner".into());
                return Err(Failure::unavailable(
                    "evidence.lock",
                    format!("resource locked: {resource} by {detail}"),
                ));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(Failure::unavailable(
        "evidence.lock",
        format!("could not acquire resource: {resource}"),
    ))
}

pub struct ResourceLease {
    pub resources: Vec<String>,
    directories: Vec<PathBuf>,
    owner: String,
}

impl ResourceLease {
    pub fn release(&mut self) {
        for directory in self.directories.drain(..).rev() {
            let current = try_json_file(&directory.join("owner.json"));
            if current.as_ref().is_some_and(|value| {
                value.get("runId").and_then(Value::as_str) == Some(&self.owner)
                    && value.get("pid").and_then(Value::as_u64) == Some(std::process::id() as u64)
            }) {
                let _ = fs::remove_dir_all(directory);
            }
        }
    }
}

impl Drop for ResourceLease {
    fn drop(&mut self) {
        self.release();
    }
}

pub fn acquire_resources_wait(
    harness: &Path,
    resources: &[String],
    owner: &str,
    timeout_ms: Option<u64>,
) -> Result<ResourceLease, Failure> {
    let mut unique = resources.to_vec();
    unique.sort();
    unique.dedup();
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(0));
    let started = Instant::now();
    loop {
        let mut acquired = Vec::new();
        let mut conflict = None;
        for resource in &unique {
            match acquire_one(harness, resource, owner) {
                Ok((directory, _)) => acquired.push(directory),
                Err(error) => {
                    conflict = Some(error);
                    break;
                }
            }
        }
        if let Some(error) = conflict {
            for directory in acquired.into_iter().rev() {
                let _ = fs::remove_dir_all(directory);
            }
            if started.elapsed() >= timeout {
                return Err(error);
            }
            thread::sleep(
                Duration::from_millis(250)
                    .min(timeout.saturating_sub(started.elapsed()))
                    .max(Duration::from_millis(1)),
            );
        } else {
            return Ok(ResourceLease {
                resources: unique,
                directories: acquired,
                owner: owner.to_string(),
            });
        }
    }
}

// Provider-neutral object listing. Kept public for read-side projections.
pub fn list_objects(root_uri: &str) -> Result<Vec<Value>, Failure> {
    let (namespace, key) = split_object_uri(root_uri)?;
    let (base_url, token) = object_store_config()?;
    let mut url = Url::parse(&format!("{base_url}/api/object/list"))
        .map_err(|error| Failure::config("objects.config", error.to_string()))?;
    url.query_pairs_mut()
        .append_pair("namespace", &namespace)
        .append_pair("prefix", &key);
    let agent = ureq::AgentBuilder::new().redirects(0).build();
    let response = match agent
        .get(url.as_str())
        .set("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(status, _)) => {
            return Err(Failure::unavailable(
                "objects.read",
                format!("Stado object storage rejected the request: {status}"),
            ));
        }
        Err(error) => {
            return Err(Failure::unavailable(
                "objects.read",
                format!("Stado object storage did not answer: {error}"),
            ));
        }
    };
    let payload: Value = response
        .into_json()
        .map_err(|error| Failure::unavailable("objects.list", error.to_string()))?;
    let objects = payload
        .get("objects")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Failure::unavailable("objects.list", "Stado list response has no objects array")
        })?;
    for item in objects {
        let uri = item.get("uri").and_then(Value::as_str).ok_or_else(|| {
            Failure::unavailable(
                "objects.list",
                "Stado list response contains an invalid object",
            )
        })?;
        let item_key = item.get("key").and_then(Value::as_str).ok_or_else(|| {
            Failure::unavailable(
                "objects.list",
                "Stado list response contains an invalid object",
            )
        })?;
        let (listed_namespace, listed_key) = split_object_uri(uri)?;
        if listed_namespace != namespace
            || listed_key != item_key
            || (listed_key != key && !listed_key.starts_with(&format!("{key}/")))
        {
            return Err(Failure::unavailable(
                "objects.list",
                "Stado list response escaped the requested Probierz prefix",
            ));
        }
    }
    Ok(objects.clone())
}

fn unsafe_url_text(value: &str) -> bool {
    value.trim() != value
        || value
            .chars()
            .any(|character| character <= '\u{1f}' || character == '\u{7f}')
        || value.contains(['\\', '%'])
        || value.split('/').any(|part| matches!(part, "." | ".."))
}

fn loopback(host: &str) -> bool {
    host == "localhost" || host == "::1" || host == "[::1]" || {
        let parts = host.split('.').collect::<Vec<_>>();
        parts.len() == 4
            && parts.iter().all(|part| {
                !part.is_empty()
                    && part.len() <= 3
                    && part.bytes().all(|byte| byte.is_ascii_digit())
                    && part.parse::<u8>().is_ok()
            })
            && parts[0] == "127"
    }
}

fn object_store_config() -> Result<(String, String), Failure> {
    let raw = std::env::var("STADO_API_URL").unwrap_or_default();
    let token = std::env::var("STADO_API_TOKEN").unwrap_or_default();
    if raw.is_empty() {
        return Err(Failure::config(
            "objects.config",
            "STADO_API_URL is required for remote object storage",
        ));
    }
    if token.is_empty() {
        return Err(Failure::config(
            "objects.config",
            "STADO_API_TOKEN is required for remote object storage",
        ));
    }
    if unsafe_url_text(&raw) {
        return Err(Failure::config(
            "objects.config",
            "STADO_API_URL contains unsafe URL syntax",
        ));
    }
    if token
        .chars()
        .any(|character| character <= '\u{1f}' || character == '\u{7f}')
    {
        return Err(Failure::config(
            "objects.config",
            "STADO_API_TOKEN contains control characters",
        ));
    }
    let parsed = Url::parse(&raw).map_err(|_| {
        Failure::config(
            "objects.config",
            "STADO_API_URL must be an absolute HTTP(S) URL",
        )
    })?;
    let unsafe_base = !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path().contains("//")
        || (parsed.path() != "/" && parsed.path().ends_with('/'));
    if unsafe_base {
        return Err(Failure::config(
            "objects.config",
            "STADO_API_URL must not contain credentials, query, fragment, or an unsafe base path",
        ));
    }
    if parsed.scheme() == "http" && !parsed.host_str().is_some_and(loopback) {
        return Err(Failure::config(
            "objects.config",
            "STADO_API_URL must use HTTPS except for authenticated loopback",
        ));
    }
    let origin = parsed.origin().ascii_serialization();
    Ok((
        format!(
            "{origin}{}",
            if parsed.path() == "/" {
                ""
            } else {
                parsed.path()
            }
        ),
        token,
    ))
}

fn split_object_uri(uri: &str) -> Result<(String, String), Failure> {
    if uri.is_empty() || unsafe_url_text(uri) {
        return Err(Failure::invalid(
            "objects.uri",
            format!("unsafe Stado object URI: {uri}"),
        ));
    }
    let parsed = Url::parse(uri)
        .map_err(|_| Failure::invalid("objects.uri", format!("invalid Stado object URI: {uri}")))?;
    if parsed.scheme() != "stado"
        || parsed.host_str() != Some("probierz")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.path().starts_with('/')
        || parsed.path().starts_with("//")
        || parsed.path().contains("//")
    {
        return Err(Failure::invalid(
            "objects.uri",
            format!("invalid Stado object URI: {uri}"),
        ));
    }
    let key = parsed.path().trim_start_matches('/').to_string();
    if !key.starts_with("capacity/") {
        return Err(Failure::invalid(
            "objects.uri",
            "Stado object URI must stay under stado://probierz/capacity/",
        ));
    }
    Ok(("probierz".into(), key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_sorts_every_object_level() {
        assert_eq!(
            canonical(&json!({"z": [3, {"b": true, "a": null}], "a": "x"})),
            r#"{"a":"x","z":[3,{"a":null,"b":true}]}"#
        );
    }

    #[test]
    fn resources_match_shared_driver_boundaries() {
        let env = BTreeMap::from([
            ("IOS_DEVICE".into(), "iPhone 17".into()),
            ("IOS_VERSION".into(), "26".into()),
        ]);
        assert_eq!(
            resources_for("mobile:ios", &env),
            vec!["device:ios:iPhone 17:26", "port:4723"]
        );
    }
}
