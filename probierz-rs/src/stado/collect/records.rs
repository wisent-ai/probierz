use serde_json::json;
use crate::stado::*;
pub(crate) fn safe_child(root: &Path, relative: &str, message: &str) -> Result<PathBuf, Failure> {
    if relative.is_empty()
        || Path::new(relative).is_absolute()
        || Path::new(relative)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(Failure::config(
            "stado.download",
            format!("{message}: {relative}"),
        ));
    }
    Ok(root.join(relative))
}

pub(crate) fn read_json(path: &Path) -> Option<Value> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

pub(crate) fn missing_evidence(job_id: &str, detail: &str) -> Value {
    failure_summary(
        &Failure::config("stado.download", format!("job={job_id}; {detail}")),
        format!("Job {job_id} completed without the required Probierz run evidence"),
    )
}

pub(crate) fn read_author_submission(harness: &Path, job_id: &str) -> Option<Value> {
    let file = harness
        .join("test-results")
        .join(".remote")
        .join(job_id)
        .join("authoring-submission.json");
    let value = read_json(&file)?;
    (value.get("schemaVersion").and_then(Value::as_u64) == Some(1)
        && value.get("jobId").and_then(Value::as_str) == Some(job_id))
    .then_some(value)
}

pub(crate) fn save_author_submission(
    harness: &Path,
    job_id: &str,
    app_id: &str,
    journey: &str,
    area: &str,
    target: &str,
    product_root: &Path,
    test_directory: &str,
    identity: &Identity,
) -> Result<PathBuf, Failure> {
    let file = harness
        .join("test-results")
        .join(".remote")
        .join(job_id)
        .join("authoring-submission.json");
    let value = json!({
        "schemaVersion": 1,
        "jobId": job_id,
        "appId": app_id,
        "journey": journey,
        "area": area,
        "target": target,
        "productRoot": product_root,
        "testDirectory": test_directory,
        "sourceSha256": identity.document.pointer("/app/sha256").cloned().unwrap_or(Value::Null),
        "harnessSha256": identity.document.pointer("/harness/sha256").cloned().unwrap_or(Value::Null),
        "installedSourceSha256": Value::Null,
    });
    write_json(&file, &value, true, true)?;
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

