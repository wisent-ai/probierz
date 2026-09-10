use serde_json::json;
use crate::stado::*;
pub(crate) fn restore_remote_authoring(
    harness: &Path,
    job_id: &str,
    retained: &Retained,
    expected_app: Option<&str>,
    required: bool,
) -> Result<Option<Value>, Failure> {
    let Some(receipt) = retained.author_receipt.as_ref() else {
        if required {
            return Err(Failure::config(
                "stado.download",
                format!("Job {job_id} completed authoring without a usable accepted-spec receipt."),
            ));
        }
        return Ok(None);
    };
    let submission = read_author_submission(harness, job_id).ok_or_else(|| Failure::config(
        "stado.download",
        format!("Job {job_id} returned an authored spec, but this checkout has no source-bound submission receipt."),
    ))?;
    let app_id = submission
        .get("appId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let journey = submission
        .get("journey")
        .and_then(Value::as_str)
        .unwrap_or("");
    let area = submission.get("area").and_then(Value::as_str).unwrap_or("");
    let target = submission
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or("");
    let product_root = PathBuf::from(
        submission
            .get("productRoot")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let test_directory = submission
        .get("testDirectory")
        .and_then(Value::as_str)
        .unwrap_or("tests");
    let expected_spec = format!(
        "{test_directory}/{area}/{journey}.probierz.spec.{}",
        product_extension(target)
    );
    let expected_registration = registration_directory(target)
        .map(|directory| {
            format!(
                "{directory}/{app_id}-{journey}{}",
                registration_extension(target)
            )
        })
        .unwrap_or_default();
    let local_application = manifest::load(harness, app_id)?;
    let local_test_directory = local_application
        .document
        .get("surfaces")
        .and_then(|value| value.get(target))
        .and_then(|value| value.get("testDirectory"))
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("tests");
    let manifest = retained.manifest.as_ref();
    let matching = receipt.get("schemaVersion").and_then(Value::as_u64) == Some(1)
        && receipt.get("appId").and_then(Value::as_str) == Some(app_id)
        && receipt.get("journey").and_then(Value::as_str) == Some(journey)
        && receipt.get("area").and_then(Value::as_str) == Some(area)
        && receipt.get("target").and_then(Value::as_str) == Some(target)
        && local_test_directory == test_directory
        && !expected_registration.is_empty()
        && receipt
            .pointer("/spec/relativePath")
            .and_then(Value::as_str)
            == Some(expected_spec.as_str())
        && receipt
            .pointer("/registration/relativePath")
            .and_then(Value::as_str)
            == Some(expected_registration.as_str())
        && receipt
            .get("mappingPaths")
            .and_then(Value::as_array)
            .map(Vec::is_empty)
            == Some(true)
        && manifest.and_then(|value| value.get("runId")) == receipt.get("runId")
        && manifest
            .and_then(|value| value.get("appId"))
            .and_then(Value::as_str)
            == Some(app_id)
        && manifest
            .and_then(|value| value.get("target"))
            .and_then(Value::as_str)
            == Some(target)
        && manifest.and_then(|value| value.pointer("/source/sha256"))
            == submission.get("sourceSha256")
        && manifest.and_then(|value| value.pointer("/harness/sha256"))
            == submission.get("harnessSha256")
        && manifest
            .and_then(|value| value.get("sourceIdentityOrigin"))
            .and_then(Value::as_str)
            == Some("submitter")
        && manifest
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            == Some("passed")
        && expected_app
            .map(|expected| expected == app_id)
            .unwrap_or(true);
    if !matching {
        return Err(Failure::config(
            "stado.download",
            format!("Job {job_id} returned authoring metadata that does not match its submitting checkout."),
        ));
    }
    let source_sha = submission.get("sourceSha256").and_then(Value::as_str);
    let harness_sha = submission.get("harnessSha256").and_then(Value::as_str);
    if source_sha.is_none()
        || receipt.get("sourceSha256").and_then(Value::as_str) != source_sha
        || harness_sha.is_none()
        || receipt.get("harnessSha256").and_then(Value::as_str) != harness_sha
    {
        return Err(Failure::config(
            "stado.download",
            format!("Job {job_id} returned an authored spec for a different source identity."),
        ));
    }
    let current_before =
        crate::authoring::app_source_identity(harness, app_id, Some(&product_root))?;
    let expected_local = submission
        .get("installedSourceSha256")
        .and_then(Value::as_str)
        .or(source_sha);
    if current_before
        .pointer("/app/sha256")
        .and_then(Value::as_str)
        .is_none()
        || current_before
            .pointer("/app/sha256")
            .and_then(Value::as_str)
            != expected_local
    {
        return Err(Failure::config(
            "stado.download",
            format!("Job {job_id} cannot publish into a checkout whose source changed after submission."),
        ));
    }
    let accepted =
        receipt
            .pointer("/spec/artifact")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Failure::config(
        "stado.download",
        format!("Job {job_id} returned an authored spec outside retained Probierz artifacts."),
    )
            })?;
    let accepted = safe_child(
        harness,
        accepted,
        "authored spec outside retained Probierz artifacts",
    )?;
    let results_root = harness.join("test-results");
    if !accepted.starts_with(&results_root) || !accepted.is_file() {
        return Err(Failure::config(
            "stado.download",
            format!("Job {job_id} returned an authored spec outside retained Probierz artifacts."),
        ));
    }
    let bytes = fs::read(&accepted)?;
    let digest = hex::encode(Sha256::digest(&bytes));
    if receipt.pointer("/spec/bytes").and_then(Value::as_u64) != Some(bytes.len() as u64)
        || receipt.pointer("/spec/sha256").and_then(Value::as_str) != Some(digest.as_str())
    {
        return Err(Failure::config(
            "stado.download",
            format!("Job {job_id} returned authored spec bytes that do not match its receipt."),
        ));
    }
    let installed = install_product_spec(
        harness,
        &product_root,
        app_id,
        journey,
        target,
        &expected_spec,
        &expected_registration,
        &bytes,
    )?;
    let current = crate::authoring::app_source_identity(harness, app_id, Some(&product_root))?;
    let source = current
        .pointer("/app/sha256")
        .cloned()
        .unwrap_or(Value::Null);
    if source.is_null() {
        return Err(Failure::config(
            "stado.download",
            format!("Job {job_id} installed an authored spec, but its product source identity is unavailable."),
        ));
    }
    let mut updated = submission.clone();
    if let Some(object) = updated.as_object_mut() {
        object.insert("installedSourceSha256".into(), source);
    }
    let source_receipt = harness
        .join("test-results")
        .join(".remote")
        .join(job_id)
        .join("authoring-submission.json");
    write_json(&source_receipt, &updated, true, true)?;
    Ok(Some(json!({
        "productSpec": installed.0,
        "registration": installed.1,
        "appManifest": installed.2,
        "authorReceipt": retained.author_receipt_file,
        "sourceReceipt": source_receipt,
    })))
}

