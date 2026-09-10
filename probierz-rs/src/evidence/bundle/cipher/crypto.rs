use serde_json::json;
use crate::evidence::*;

pub(crate) type Aes256Ctr = Ctr32BE<Aes256>;

#[cfg(unix)]
pub(crate) fn file_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    metadata.mode() & 0o777
}

#[cfg(not(unix))]
pub(crate) fn file_mode(_metadata: &fs::Metadata) -> u32 {
    0o600
}

#[cfg(unix)]
pub(crate) fn apply_mode(path: &Path, mode: u32) -> Result<(), Failure> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn apply_mode(_path: &Path, _mode: u32) -> Result<(), Failure> {
    Ok(())
}

pub(crate) fn key_from_file(path: Option<&Path>) -> Result<[u8; 32], Failure> {
    let path = path
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE").map(PathBuf::from))
        .ok_or_else(|| {
            Failure::config(
                "evidence.protect",
                "artifact encryption key file is required",
            )
        })?;
    let raw = fs::read(path)?;
    let decoded = if raw.len() == 32 {
        raw
    } else {
        let text = String::from_utf8_lossy(&raw).trim().to_string();
        if text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            hex::decode(text)
                .map_err(|error| Failure::config("evidence.protect", error.to_string()))?
        } else {
            BASE64
                .decode(text)
                .map_err(|error| Failure::config("evidence.protect", error.to_string()))?
        }
    };
    decoded.try_into().map_err(|_| {
        Failure::config(
            "evidence.protect",
            "artifact encryption key must contain exactly 32 bytes, hex, or base64",
        )
    })
}

pub(crate) fn retention_days(document: &Value, kind: &str) -> Result<f64, Failure> {
    let name = match kind {
        "pull-request" => "pullRequestDays",
        "nightly" => "nightlyDays",
        "release" => "releaseDays",
        "synthetic" => "syntheticDays",
        _ => "adhocDays",
    };
    let retain = document.pointer("/artifacts/retain");
    let value = retain
        .and_then(|item| item.get(name))
        .or_else(|| retain.and_then(|item| item.get("pullRequestDays")))
        .and_then(|item| item.as_f64().or_else(|| item.as_i64().map(|n| n as f64)))
        .unwrap_or(14.0);
    if !value.is_finite() || value <= 0.0 {
        return Err(Failure::config(
            "evidence.retention",
            format!("invalid artifact retention for {kind}"),
        ));
    }
    Ok(value)
}

pub(crate) fn expires_at(started_at: &str, days: f64) -> Result<String, Failure> {
    let parsed = DateTime::parse_from_rfc3339(started_at).map_err(|_| {
        Failure::invalid(
            "evidence.retention",
            format!("invalid run timestamp: {started_at}"),
        )
    })?;
    let milliseconds = (days * 86_400_000.0) as i64;
    Ok((parsed + chrono::Duration::milliseconds(milliseconds))
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true))
}

pub(crate) fn encoded_header(header: &Value) -> Result<Vec<u8>, Failure> {
    let body = format!("{}\n", serde_json::to_string(header)?).into_bytes();
    let length: u32 = body.len().try_into().map_err(|_| {
        Failure::invalid("evidence.protect", "invalid evidence bundle header length")
    })?;
    let mut prefix = Vec::with_capacity(MAGIC.len() + 4 + body.len());
    prefix.extend_from_slice(MAGIC);
    prefix.extend_from_slice(&length.to_be_bytes());
    prefix.extend_from_slice(&body);
    Ok(prefix)
}

pub(crate) fn read_header(file: &Path) -> Result<(Value, usize, Vec<u8>), Failure> {
    let mut input = File::open(file)?;
    let mut prefix = vec![0u8; MAGIC.len() + 4];
    input.read_exact(&mut prefix).map_err(|_| {
        Failure::invalid(
            "evidence.restore",
            "not a Probierz encrypted evidence bundle",
        )
    })?;
    if &prefix[..MAGIC.len()] != MAGIC {
        return Err(Failure::invalid(
            "evidence.restore",
            "not a Probierz encrypted evidence bundle",
        ));
    }
    let length = u32::from_be_bytes(prefix[MAGIC.len()..].try_into().map_err(|_| {
        Failure::invalid("evidence.restore", "invalid evidence bundle header length")
    })?) as usize;
    if length == 0 || length > 1024 * 1024 {
        return Err(Failure::invalid(
            "evidence.restore",
            "invalid evidence bundle header length",
        ));
    }
    let mut body = vec![0u8; length];
    input
        .read_exact(&mut body)
        .map_err(|_| Failure::invalid("evidence.restore", "truncated evidence bundle header"))?;
    let header: Value = serde_json::from_slice(&body)?;
    prefix.extend_from_slice(&body);
    Ok((header, prefix.len(), prefix))
}

pub(crate) fn remove_plaintext_source(
    source: &Path,
    manifest_path: &Path,
    retention_kind: &str,
    protected: &Value,
) -> Result<(), Failure> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name() == "run-manifest.json" {
            continue;
        }
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    let mut current = json_file(manifest_path)?;
    let object = current
        .as_object_mut()
        .ok_or_else(|| Failure::config("evidence.protect", "run manifest is not an object"))?;
    if object.get("kind").is_none_or(Value::is_null) {
        object.insert("kind".into(), json!(retention_kind));
    }
    let mut protection = protected.clone();
    protection
        .as_object_mut()
        .ok_or_else(|| Failure::config("evidence.protect", "protection is not an object"))?
        .insert("plaintextRemoved".into(), json!(true));
    object.insert("protection".into(), protection);
    object.insert("plaintextArtifactsRemovedAt".into(), json!(now_iso()));
    let temporary = manifest_path.with_extension(format!(
        "json.tmp-{}-{}",
        std::process::id(),
        Utc::now().timestamp_millis()
    ));
    write_new_json(&temporary, &current, true)?;
    fs::rename(temporary, manifest_path)?;
    Ok(())
}


