use serde_json::json;
use crate::evidence::*;
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

