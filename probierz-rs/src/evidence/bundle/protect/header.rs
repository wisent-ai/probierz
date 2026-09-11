//! The header of a new encrypted bundle: the run's identities, its evidence level,
//! the journeys it covered, its retention, its key fingerprint and its content index.

use crate::evidence::*;
use serde_json::json;

pub(super) struct HeaderInputs<'a> {
    pub(super) run: &'a Value,
    pub(super) document: &'a Value,
    pub(super) current: &'a Value,
    pub(super) app_id: &'a str,
    pub(super) run_id: &'a str,
    pub(super) retention_kind: &'a str,
    pub(super) days: f64,
    pub(super) key: &'a [u8],
    pub(super) index_hash: &'a str,
    pub(super) scan: &'a Value,
    pub(super) entries: &'a [Value],
    pub(super) nonce: &'a [u8; 12],
}

pub(super) fn bundle_header(inputs: HeaderInputs<'_>) -> Result<Value, Failure> {
    let HeaderInputs {
        run,
        document,
        current,
        app_id,
        run_id,
        retention_kind,
        days,
        key,
        index_hash,
        scan,
        entries,
        nonce,
    } = inputs;
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
        "keyFingerprintSha256": sha256_bytes(key),
        "contentIndexSha256": index_hash,
        "secretScan": {
            "passed": scan.get("passed").cloned().unwrap_or(Value::Null),
            "scannedFiles": scan.get("scannedFiles").cloned().unwrap_or(Value::Null),
            "skippedBinary": scan.get("skippedBinary").cloned().unwrap_or(Value::Null),
        },
        "files": entries.len(),
        "plaintextBytes": entries.iter().filter_map(|entry| entry.get("bytes").and_then(Value::as_u64)).sum::<u64>(),
    });
    Ok(header)
}
