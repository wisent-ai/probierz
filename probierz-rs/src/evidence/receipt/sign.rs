//! Signing an evidence receipt: the arguments it needs, the runs it covers, the
//! checks they must pass, and the signed document written under test-results.

mod checks;

use crate::evidence::*;
use serde_json::json;

use checks::{check_artifacts, check_runs, exact_builds, journey_coverage};

/// What `probierz evidence receipt create` asks to sign, as its arguments
/// give it; each required one is refused by name when absent.
pub struct ReceiptRequest<'a> {
    pub app_id: Option<&'a str>,
    pub release: Option<&'a str>,
    pub expected_harness: Option<&'a str>,
    pub expected_source: Option<&'a str>,
    pub runs_csv: Option<&'a str>,
    pub journeys_csv: Option<&'a str>,
    pub minimum: Option<&'a str>,
}

pub fn receipt(harness: &Path, request: ReceiptRequest<'_>) -> Answer {
    let ReceiptRequest {
        app_id,
        release,
        expected_harness,
        expected_source,
        runs_csv,
        journeys_csv,
        minimum,
    } = request;
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
    // The level is the release policy's; none is assumed, and it is read by
    // the one rank the gate, status and publication read.
    let minimum = minimum.ok_or_else(|| {
        Failure::invalid(
            "evidence.receipt",
            "--minimum is required: the lowest evidence level (E<n>) every signed run must reach, as the app's release policy states it",
        )
    })?;
    if crate::gate::evidence_rank(minimum).is_none() {
        return Err(Failure::invalid(
            "evidence.receipt",
            format!("--minimum {minimum:?} is not an evidence level: write E followed by its rank, as the app's release policy does"),
        ));
    }
    let key_file = std::env::var_os("PROBIERZ_RECEIPT_PRIVATE_KEY_FILE").ok_or_else(|| {
        Failure::config(
            "evidence.receipt",
            "PROBIERZ_RECEIPT_PRIVATE_KEY_FILE is required",
        )
    })?;
    let loaded = manifest::load(harness, app_id)?;
    let document = loaded.json()?;
    let source_runs = run_ids
        .iter()
        .map(|run_id| get_run(harness, app_id, run_id))
        .collect::<Result<Vec<_>, _>>()?;
    let normalized = source_runs
        .iter()
        .map(|run| signed_receipt_run(run, &document))
        .collect::<Result<Vec<_>, _>>()?;
    let current = crate::authoring::app_source_identity(harness, app_id, None)?;
    let mut errors = Vec::<String>::new();
    if current.pointer("/harness/sha256").and_then(Value::as_str) != Some(expected_harness) {
        errors.push(
            "expected harness source is stale relative to the current Probierz checkout".into(),
        );
    }
    if current.pointer("/app/sha256").and_then(Value::as_str) != Some(expected_source) {
        errors.push("expected app source is stale relative to the current product checkout".into());
    }
    let scans = check_artifacts(harness, app_id, &source_runs, &normalized, &mut errors)?;
    check_runs(
        &normalized,
        expected_harness,
        expected_source,
        minimum,
        &mut errors,
    );
    let builds = exact_builds(&normalized, &mut errors);
    let (covered, required, missing) = journey_coverage(&normalized, journeys_csv, &mut errors);
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
        "schemaVersion": super::RECEIPT_SCHEMA_VERSION, "kind": "probierz-evidence-receipt", "appId": app_id, "release": release,
        "expectedHarnessSha": expected_harness, "expectedSourceSha": expected_source, "builds": builds,
        "productId": document.get("productId").cloned().unwrap_or_else(|| json!(app_id)),
        "artifactPolicy": {
            "retain": document.pointer("/artifacts/retain").cloned().unwrap_or_else(|| json!({})),
            "redact": redact_policy,
            // A manifest that declares no PII classification is signed as
            // declaring none (null), not as one nobody chose.
            "pii": document.pointer("/artifacts/pii"),
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
        .join(segment(app_id, "app id")?)
        .join(segment(release, "release")?)
        .join(format!("{receipt_id}.json"));
    write_new_json(&file, &receipt_value, true)?;
    let result = json!({ "file": file.to_string_lossy(), "receiptId": receipt_id, "receipt": receipt_value });
    print_json(&result)?;
    // A failed verdict is still a signed, persisted receipt, and the answer
    // above names it; the refusal is returned rather than ending the
    // process, because the MCP server runs this command in its own process
    // and must go on answering after a negative receipt.
    if !errors.is_empty() {
        return Err(Failure::new(
            "evidence.receipt",
            crate::failure::Code::Refused,
            format!(
                "receipt {receipt_id} at {} records a failed verdict: {}",
                file.display(),
                json!(errors)
            ),
        ));
    }
    Ok(())
}
