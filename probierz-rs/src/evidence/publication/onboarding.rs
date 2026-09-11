//! Publishing a first-use onboarding journey from a signed, trusted, passing receipt.

mod assets;

use crate::evidence::*;
use serde_json::json;

use assets::{bound_assets, AssetContext};

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
pub(crate) fn create_onboarding_publication(
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
    let assets = bound_assets(
        run,
        &catalog,
        &AssetContext {
            run_id,
            screen_id,
            evidence,
            default_captured: &default_captured,
            verified_at: &verified_at,
        },
    )?;
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
