use serde_json::json;
use crate::evidence::*;
pub(crate) fn write_json(path: &Path, value: &Value) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    apply_mode(path, 0o600)?;
    Ok(())
}

pub(crate) fn write_new_json(path: &Path, value: &Value, mode_600: bool) -> Result<(), Failure> {
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

pub(crate) fn segment(value: &str, fallback: &str) -> String {
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

pub(crate) fn policy_conditions(run: &Value, document: &Value) -> Value {
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

pub(crate) fn signed_protection(protection: Option<&Value>) -> Value {
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

pub(crate) fn journey_identities(run: &Value, document: &Value) -> Vec<Value> {
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

pub(crate) fn signed_media(run: &Value) -> Vec<Value> {
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

pub(crate) fn signed_receipt_run(run: &Value, document: &Value) -> Value {
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

