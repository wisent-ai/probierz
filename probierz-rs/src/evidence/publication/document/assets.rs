use serde_json::json;
use crate::evidence::*;

/// Every asset a publication registers, checked one at a time: its kind is
/// declared, its artifact is the one the run signed, and no artifact is
/// registered twice. The list comes back sorted by artifact id so two
/// publications of the same run are byte-identical.
pub(crate) fn publication_assets(
    attempt_id: &str,
    evidence: &str,
    run: &Value,
    assets: &Value,
    policy: &Value,
    signed_media: &HashMap<&str, &Value>,
) -> Result<Vec<Value>, Failure> {
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
    Ok(publication_assets)
}
