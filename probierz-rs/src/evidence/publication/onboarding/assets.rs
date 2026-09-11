//! The assets an onboarding publication carries: every catalogue entry bound to a
//! signed receipt artifact, with its kind, redaction status, immutable storage URL
//! and capture time checked, and no artifact published twice.

use crate::evidence::*;
use serde_json::json;

pub(super) struct AssetContext<'a> {
    pub(super) run_id: &'a str,
    pub(super) screen_id: &'a str,
    pub(super) evidence: &'a str,
    pub(super) default_captured: &'a str,
    pub(super) verified_at: &'a str,
}

pub(super) fn bound_assets(
    run: &Value,
    catalog: &Value,
    context: &AssetContext<'_>,
) -> Result<Vec<Value>, Failure> {
    let AssetContext {
        run_id,
        screen_id,
        evidence,
        default_captured,
        verified_at,
    } = *context;
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
            None => default_captured.to_string(),
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
    Ok(assets)
}
