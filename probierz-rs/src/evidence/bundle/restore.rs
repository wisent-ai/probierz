use serde_json::json;
use crate::evidence::*;
pub fn restore(
    harness: &Path,
    file: Option<&Path>,
    destination: Option<&Path>,
    key_file: Option<&Path>,
) -> Answer {
    let file = file.ok_or_else(|| {
        Failure::invalid("evidence.restore", "restore needs a bundle and destination")
    })?;
    let destination = destination.ok_or_else(|| {
        Failure::invalid("evidence.restore", "restore needs a bundle and destination")
    })?;
    let result = restore_bundle(file, destination, key_file);
    match result {
        Ok(value) => {
            let app = value.get("appId").and_then(Value::as_str);
            let run = value.get("runId").and_then(Value::as_str);
            let _ = audit_access(
                harness,
                "artifact.restore",
                "allowed",
                app,
                run,
                Some(file),
                json!({
                    "destination": value.get("destination").cloned().unwrap_or(Value::Null),
                    "files": value.get("files").cloned().unwrap_or(Value::Null),
                }),
            );
            print_json(&value)
        }
        Err(error) => {
            let _ = audit_access(
                harness,
                "artifact.restore",
                "denied",
                None,
                None,
                Some(file),
                json!({
                    "destination": destination.to_string_lossy(), "error": error.detail,
                }),
            );
            Err(error)
        }
    }
}

pub(crate) fn restore_bundle(
    file: &Path,
    destination: &Path,
    key_file: Option<&Path>,
) -> Result<Value, Failure> {
    if destination.is_dir() && fs::read_dir(destination)?.next().is_some() {
        return Err(Failure::invalid(
            "evidence.restore",
            "restore destination must be empty",
        ));
    }
    fs::create_dir_all(destination)?;
    apply_mode(destination, 0o700)?;
    let key = key_from_file(key_file)?;
    let (header, offset, aad) = read_header(file)?;
    if header.get("algorithm").and_then(Value::as_str) != Some("AES-256-GCM") {
        return Err(Failure::invalid(
            "evidence.restore",
            format!(
                "unsupported evidence algorithm: {}",
                header
                    .get("algorithm")
                    .and_then(Value::as_str)
                    .unwrap_or("undefined")
            ),
        ));
    }
    if header.get("keyFingerprintSha256").and_then(Value::as_str) != Some(&sha256_bytes(&key)) {
        return Err(Failure::invalid(
            "evidence.restore",
            "artifact encryption key fingerprint mismatch",
        ));
    }
    let nonce_bytes = BASE64
        .decode(
            header
                .get("nonce")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .map_err(|error| Failure::invalid("evidence.restore", error.to_string()))?;
    let (payload, plaintext_bytes) =
        decrypt_bundle_payload(file, destination, &key, &nonce_bytes, &aad, offset)?;
    let mut plaintext = File::open(payload.path())?;
    let mut index_size = [0u8; 4];
    plaintext
        .read_exact(&mut index_size)
        .map_err(|_| Failure::invalid("evidence.restore", "truncated evidence index"))?;
    let length = u32::from_be_bytes(index_size) as usize;
    if length == 0 || length > 64 * 1024 * 1024 {
        return Err(Failure::invalid(
            "evidence.restore",
            "invalid evidence index length",
        ));
    }
    let mut index_bytes = vec![0u8; length];
    plaintext
        .read_exact(&mut index_bytes)
        .map_err(|_| Failure::invalid("evidence.restore", "truncated evidence index"))?;
    let index: Value = serde_json::from_slice(&index_bytes)?;
    let entries = index
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let root = absolute(destination)?;
    let mut cursor = 4u64 + length as u64;
    let mut buffer = vec![0u8; 128 * 1024];
    for entry in &entries {
        let member = entry
            .get("file")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if member.is_empty()
            || Path::new(member).is_absolute()
            || member.split('/').any(|part| part == "..")
        {
            return Err(Failure::invalid(
                "evidence.restore",
                format!("unsafe evidence member: {member}"),
            ));
        }
        let output = absolute(&root.join(member))?;
        if !output.starts_with(&root) {
            return Err(Failure::invalid(
                "evidence.restore",
                format!("unsafe evidence member: {member}"),
            ));
        }
        let count = entry.get("bytes").and_then(Value::as_u64).unwrap_or(0);
        cursor = cursor.checked_add(count).ok_or_else(|| {
            Failure::invalid(
                "evidence.restore",
                "encrypted evidence payload has trailing or missing bytes",
            )
        })?;
        if cursor > plaintext_bytes {
            return Err(Failure::invalid(
                "evidence.restore",
                "encrypted evidence payload has trailing or missing bytes",
            ));
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
            apply_mode(parent, 0o700)?;
        }
        let mut target = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)?;
        let mut remaining = count;
        let mut digest = Sha256::new();
        while remaining > 0 {
            let wanted = remaining.min(buffer.len() as u64) as usize;
            plaintext.read_exact(&mut buffer[..wanted]).map_err(|_| {
                Failure::invalid(
                    "evidence.restore",
                    "encrypted evidence payload has trailing or missing bytes",
                )
            })?;
            digest.update(&buffer[..wanted]);
            target.write_all(&buffer[..wanted])?;
            remaining -= wanted as u64;
        }
        drop(target);
        apply_mode(
            &output,
            entry
                .get("mode")
                .and_then(Value::as_u64)
                .filter(|mode| *mode != 0)
                .unwrap_or(0o600) as u32,
        )?;
        if hex::encode(digest.finalize())
            != entry
                .get("sha256")
                .and_then(Value::as_str)
                .unwrap_or_default()
        {
            return Err(Failure::invalid(
                "evidence.restore",
                format!("restored evidence hash mismatch: {member}"),
            ));
        }
    }
    if cursor != plaintext_bytes {
        return Err(Failure::invalid(
            "evidence.restore",
            "encrypted evidence payload has trailing or missing bytes",
        ));
    }
    Ok(json!({
        "appId": header.get("appId").cloned().unwrap_or(Value::Null),
        "runId": header.get("runId").cloned().unwrap_or(Value::Null),
        "destination": destination.to_string_lossy(),
        "files": entries.len(),
        "authenticated": true,
    }))
}

