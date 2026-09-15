//! Where an SEO report goes and how it is identified: the signing key,
//! the report id, the destination path, and the single write.
//!
//! Kept apart from the evaluation itself because none of it depends on
//! what was evaluated — it takes the finished payload and nothing else.

use crate::authoring::*;
use serde_json::json;

/// Characters of the payload digest used as the report id.
const REPORT_ID_LENGTH: usize = 24;

/// Reports are owner read/write only: they quote crawled pages and
/// carry a signature.
const REPORT_MODE: u32 = 0o600;

/// The Ed25519 key a report is signed with: the explicit value, the
/// environment's value, or the named file, in that order. No key means
/// an unsigned report, which the profile may refuse.
pub(crate) fn signing_key(
    private_key: Option<&str>,
    private_key_file: Option<&Path>,
) -> Result<Option<Vec<u8>>, Failure> {
    if let Some(value) = private_key.filter(|value| !value.trim().is_empty()) {
        return Ok(Some(value.as_bytes().to_vec()));
    }
    if let Ok(value) = std::env::var("PROBIERZ_SEO_RECEIPT_PRIVATE_KEY") {
        if !value.trim().is_empty() {
            return Ok(Some(value.into_bytes()));
        }
    }
    let selected = private_key_file
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("PROBIERZ_RECEIPT_PRIVATE_KEY_FILE").map(PathBuf::from));
    match selected.as_deref() {
        Some(file) => Ok(Some(fs::read(file)?)),
        None => Ok(None),
    }
}

/// The report id: a digest of the payload, and of the signature when
/// the report is signed, so two reports over the same pages are still
/// distinguishable by their signing.
pub(crate) fn report_id(payload: &JsonValue, signing: Option<&JsonValue>) -> String {
    let digest = match signing {
        Some(signing) => Sha256::digest(
            format!(
                "{}\n{}",
                canonical_json(payload),
                signing["signature"].as_str().unwrap_or_default()
            )
            .as_bytes(),
        ),
        None => Sha256::digest(payload.to_string().as_bytes()),
    };
    hex::encode(digest)[..REPORT_ID_LENGTH].to_string()
}

/// Where the report goes: the named path, which must be JSON, or a
/// timestamped file under the harness's own results directory.
pub(crate) fn report_file(
    harness: &Path,
    app_id: &str,
    output: Option<&Path>,
) -> Result<PathBuf, Failure> {
    let Some(path) = output else {
        return Ok(harness
            .join("test-results/seo")
            .join(app_id)
            .join(
                chrono::Utc::now()
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                    .replace([':', '.'], "-"),
            )
            .join("seo-evaluation.json"));
    };
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
        != Some("json")
    {
        return Err(Failure::invalid(
            "seo-evaluate",
            "SEO output path must end in .json",
        ));
    }
    Ok(path)
}

/// Write the report exactly once: a path that already exists is an
/// error, not an overwrite, because a report is evidence.
pub(crate) fn write_payload(file: &Path, payload: &JsonValue) -> Result<(), Failure> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(file)?;
    destination.set_permissions(fs::Permissions::from_mode(REPORT_MODE))?;
    destination.write_all(serde_json::to_string_pretty(payload)?.as_bytes())?;
    destination.write_all(b"\n")?;
    Ok(())
}

/// The summary the command answers with: where the report is, whether
/// it passed, and what it measured.
pub(crate) fn summary(
    file: &Path,
    report_id: &str,
    payload: &JsonValue,
    quality: f64,
    signing: Option<JsonValue>,
) -> JsonValue {
    json!({
        "file": file.to_string_lossy(), "reportId": report_id,
        "pass": payload.pointer("/verdict/pass").and_then(JsonValue::as_bool).unwrap_or(false),
        "searchEligibility": payload.pointer("/verdict/searchEligibility").cloned().unwrap_or(JsonValue::Null),
        "searchQuality": quality,
        "productionOutcome": payload.pointer("/verdict/productionOutcome").cloned().unwrap_or(JsonValue::Null),
        "blockers": payload.pointer("/verdict/blockers").cloned().unwrap_or(json!([])),
        "signing": signing.map(|value| json!({
            "algorithm": value["algorithm"],
            "publicKeyFingerprintSha256": value["publicKeyFingerprintSha256"],
            "payloadSha256": value["payloadSha256"]
        }))
    })
}
