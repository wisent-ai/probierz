use serde_json::json;
use crate::evidence::*;
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

