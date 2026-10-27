use crate::evidence::*;
use serde_json::json;

/// How an asset's redaction was settled, as a registration states it.
#[derive(serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Redaction {
    VerifiedRedacted,
    NotApplicable,
}

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
    let mut publication_assets: Vec<(String, Value)> = Vec::new();
    let rejected = |message: String| {
        Failure::invalid("evidence.publication", format!("publication rejected: {message}"))
    };
    let registrations = assets
        .as_array()
        .ok_or_else(|| rejected(format!("assets must be a list, not {assets}")))?;
    let screen = require_string(policy.get("screenId"), "the journey policy states no screenId")?;
    for (index, registration) in registrations.iter().enumerate() {
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
        // The kind the receipt signed, allowed by the journey policy (whose
        // kinds the manifest validates) and supported by the run's driver.
        let kind = require_string(
            media.get("artifactKind"),
            &format!("assets.{index} signed evidence states no artifact kind"),
        )?;
        require(
            policy
                .get("artifactKinds")
                .and_then(Value::as_array)
                .is_some_and(|kinds| kinds.iter().any(|value| value.as_str() == Some(kind))),
            format!("assets.{index} kind {kind} is not allowed by the journey policy"),
        )?;
        let target = require_string(run.get("target"), "the signed run states no target")?;
        require(
            manifest::target_supports_artifact_kind(target, kind),
            format!("driver {target} does not support {kind}"),
        )?;
        require(
            registration
                .get("kind")
                .is_none_or(|value| value.as_str() == Some(kind)),
            format!("assets.{index}.kind conflicts with signed evidence"),
        )?;
        let content = require_string(
            registration.get("contentSha256"),
            &format!("assets.{index}.contentSha256 is required"),
        )?;
        require(is_sha256(content), format!("assets.{index}.contentSha256 is required"))?;
        require(
            media.get("contentSha256").and_then(Value::as_str) == Some(content),
            format!("assets.{index}.contentSha256 does not match the signed receipt"),
        )?;
        let storage_message = format!(
            "assets.{index}.storageUrl must be immutable HTTPS without credentials, query, or fragment"
        );
        let storage = require_string(registration.get("storageUrl"), &storage_message)?;
        require(immutable_storage_url(storage, true), storage_message)?;
        let unsupported = format!("assets.{index}.redactionStatus is unsupported");
        let redaction_text = require_string(registration.get("redactionStatus"), &unsupported)?;
        let redaction = serde_json::from_value::<Redaction>(json!(redaction_text))
            .map_err(|cause| rejected(format!("{unsupported}: {redaction_text} ({cause})")))?;
        if policy.get("redactionRequired").and_then(Value::as_bool) == Some(true) {
            require(
                redaction == Redaction::VerifiedRedacted,
                format!("assets.{index} lacks required redaction verification"),
            )?;
        }
        let verified_text = require_string(
            registration.get("verifiedAt"),
            &format!("assets.{index}.verifiedAt is required"),
        )?;
        let verified = parse_iso(verified_text, &format!("assets.{index}.verifiedAt"))?;
        let captured_text = require_string(
            media.get("capturedAt"),
            &format!("assets.{index} signed evidence states no capturedAt"),
        )?;
        let captured = parse_iso(captured_text, &format!("assets.{index}.capturedAt"))?;
        require(
            verified >= captured,
            format!("assets.{index}.verifiedAt predates capture"),
        )?;
        let fields = json!({
            "attemptId": attempt_id, "screenId": screen, "kind": kind,
            "storageUrl": storage, "contentSha256": content,
            "bytes": media.get("bytes").and_then(Value::as_f64).map(js_number),
            "evidenceLevel": evidence, "redactionStatus": redaction_text, "capturedAt": captured_text, "verifiedAt": verified_text,
        });
        let artifact_id = sha256_bytes(canonical(&fields).as_bytes());
        // The artifact id leads the asset, then the fields it digests.
        let mut asset = Map::new();
        asset.insert("artifactId".into(), json!(artifact_id));
        if let Value::Object(fields) = fields {
            asset.extend(fields);
        }
        publication_assets.push((artifact_id, Value::Object(asset)));
    }
    publication_assets.sort_by(|(left, _), (right, _)| left.cmp(right));
    Ok(publication_assets.into_iter().map(|(_, asset)| asset).collect())
}
