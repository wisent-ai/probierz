use crate::gate::*;

/// The signed receipt a release gate stands on: verified against the trust
/// the caller named, then checked against the application, the builds and
/// the runs it claims to cover. Every disagreement is pushed onto `errors`
/// rather than lowering the bar; the verified document is returned as the
/// verdict's own evidence.
#[allow(clippy::too_many_arguments)]
pub(crate) fn release_receipt(
    args: &GateArgs,
    app: &manifest::Manifest,
    runs: &[Run],
    run_ids: &[String],
    builds: &Map<String, Value>,
    bundle_hashes: &HashMap<String, String>,
    expected_source: Option<&str>,
    require_protected: bool,
    errors: &mut Vec<String>,
) -> Result<Value, Failure> {
    if args.mode != "release" {
        return Ok(Value::Null);
    }
    if args.release.as_deref().unwrap_or("").is_empty() {
        errors.push("release ID is required".to_string());
    }
    let Some(receipt_file) = args.receipt.as_deref() else {
        errors.push("signed receipt is required".to_string());
        return Ok(Value::Null);
    };
    let verified = match crate::evidence::verify_receipt_value(
        receipt_file,
        args.public_key.as_deref(),
        args.fingerprint.as_deref(),
    ) {
        Ok(verified) => verified,
        Err(error) => {
            errors.push(format!("receipt verification failed: {}", error.detail));
            return Ok(Value::Null);
        }
    };
    if !truthy(property(&verified, "valid")) {
        errors.push("receipt signature, trust, or payload hash is invalid".to_string());
    }
    if string_property(&verified, "appId").as_deref() != Some(args.app_id.as_str()) {
        errors.push(format!(
            "receipt app ID {} does not match {}",
            js_display(property(&verified, "appId")),
            args.app_id
        ));
    }
    if !js_strict_optional_string(property(&verified, "release"), args.release.as_deref()) {
        errors.push(format!(
            "receipt release {} does not match {}",
            js_display(property(&verified, "release")),
            args.release.as_deref().unwrap_or("undefined")
        ));
    }
    if string_property(&verified, "expectedHarnessSha").as_deref()
        != Some(args.expected_harness_sha.as_str())
    {
        errors.push("receipt harness source SHA-256 does not match".to_string());
    }
    if !js_strict_optional_string(property(&verified, "expectedSourceSha"), expected_source) {
        errors.push("receipt app source SHA-256 does not match".to_string());
    }
    if canonical(property(&verified, "builds").unwrap_or(&Value::Null))
        != canonical(&Value::Object(builds.clone()))
    {
        errors.push("receipt build identities do not match".to_string());
    }
    let receipt_run_ids = value_strings(property(&verified, "runIds"));
    if !same_set(&receipt_run_ids, run_ids) {
        errors.push("receipt run IDs do not match gate run IDs".to_string());
    }
    if !truthy(property(&verified, "verdict").and_then(|verdict| property(verdict, "passed"))) {
        errors.push("receipt verdict is not passed".to_string());
    }
    let signed_runs = value_array(property(&verified, "runs"));
    let document = serde_json::to_value(&app.document)?;
    for run in runs {
        let signed = signed_runs
            .iter()
            .find(|candidate| string_property(candidate, "runId").as_deref() == Some(run.run_id.as_str()));
        let local =
            crate::evidence::signed_receipt_run_value(&receipt_run_value(run), &document);
        if signed.map(canonical) != Some(canonical(&local)) {
            errors.push(format!(
                "{}: local policy evidence differs from the signed receipt",
                run.run_id
            ));
            continue;
        }
        if require_protected
            && bundle_hashes.get(&run.run_id).map(String::as_str)
                != signed
                    .and_then(|item| property(item, "protection"))
                    .and_then(|item| string_property(item, "sha256"))
                    .as_deref()
        {
            errors.push(format!(
                "{}: encrypted bundle does not match the signed receipt",
                run.run_id
            ));
        }
    }
    Ok(verified)
}
