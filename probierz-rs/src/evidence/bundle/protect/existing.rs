//! Reusing an encrypted bundle that already exists for the run: its identity, key
//! fingerprint and content index must match the plaintext exactly as it stands.

use crate::evidence::*;
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub(super) fn reuse_existing_bundle(
    destination: &Path,
    source: &Path,
    manifest_path: &Path,
    app_id: &str,
    run_id: &str,
    retention_kind: &str,
    key: &[u8],
    index_hash: &str,
    remove_source: bool,
) -> Result<Value, Failure> {
    let (header, _, _) = read_header(destination)?;
    if header.get("runId").and_then(Value::as_str) != Some(run_id)
        || header.get("appId").and_then(Value::as_str) != Some(app_id)
    {
        return Err(Failure::invalid(
            "evidence.protect",
            "encrypted bundle identity mismatch",
        ));
    }
    if header.get("keyFingerprintSha256").and_then(Value::as_str) != Some(&sha256_bytes(key)) {
        return Err(Failure::invalid(
            "evidence.protect",
            "artifact encryption key fingerprint mismatch",
        ));
    }
    let Some(existing_index) = header.get("contentIndexSha256").and_then(Value::as_str) else {
        return Err(Failure::invalid(
            "evidence.protect",
            "existing encrypted bundle predates source-integrity metadata",
        ));
    };
    if existing_index != index_hash {
        return Err(Failure::invalid(
            "evidence.protect",
            "plaintext artifacts changed after the encrypted bundle was created",
        ));
    }
    let protected = json!({
        "file": destination.to_string_lossy(),
        "bytes": fs::metadata(destination)?.len(),
        "sha256": sha256_file(destination)?,
        "contentIndexSha256": existing_index,
        "keyFingerprintSha256": header.get("keyFingerprintSha256").cloned().unwrap_or(Value::Null),
        "expiresAt": header.get("expiresAt").cloned().unwrap_or(Value::Null),
        "retentionDays": header.get("retentionDays").cloned().unwrap_or(Value::Null),
        "files": header.get("files").cloned().unwrap_or(Value::Null),
        "secretScan": header.get("secretScan").cloned().unwrap_or(Value::Null),
        "plaintextRemoved": remove_source,
        "reused": true,
    });
    if remove_source {
        remove_plaintext_source(source, &manifest_path, retention_kind, &protected)?;
    }
    return Ok(protected);
}
