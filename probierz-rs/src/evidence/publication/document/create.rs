use serde_json::json;
use crate::evidence::*;
pub(crate) fn create_publication(
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
    let publication_assets =
        publication_assets(attempt_id, evidence, run, assets, policy, &signed_media)?;
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
