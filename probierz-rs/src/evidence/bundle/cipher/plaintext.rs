use crate::evidence::*;
pub(crate) struct TemporaryPlaintext(PathBuf);

impl TemporaryPlaintext {
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryPlaintext {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) fn decrypt_bundle_payload(
    file: &Path,
    destination: &Path,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    offset: usize,
) -> Result<(TemporaryPlaintext, u64), Failure> {
    let total_bytes = fs::metadata(file)?.len();
    let overhead = (offset as u64)
        .checked_add(TAG_BYTES as u64)
        .ok_or_else(|| {
            Failure::invalid("evidence.restore", "truncated encrypted evidence bundle")
        })?;
    let ciphertext_bytes = total_bytes.checked_sub(overhead).ok_or_else(|| {
        Failure::invalid("evidence.restore", "truncated encrypted evidence bundle")
    })?;
    let (mut stream, mut state, mask) = gcm_state(key, nonce, aad, "evidence.restore")?;
    let mut tail = Vec::with_capacity(16);
    let temporary = TemporaryPlaintext(
        destination
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!(
                ".probierz-restore-{}-{}",
                std::process::id(),
                Utc::now().timestamp_millis(),
            )),
    );
    let mut plaintext = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary.path())?;
    apply_mode(temporary.path(), 0o600)?;
    let mut input = File::open(file)?;
    input.seek(SeekFrom::Start(offset as u64))?;
    let mut encrypted = (&mut input).take(ciphertext_bytes);
    let mut buffer = vec![0u8; 128 * 1024];
    loop {
        let count = encrypted.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        ghash_feed(&mut state, &mut tail, &buffer[..count]);
        stream
            .try_apply_keystream(&mut buffer[..count])
            .map_err(|_| {
                Failure::invalid(
                    "evidence.restore",
                    "encrypted evidence payload is too large",
                )
            })?;
        plaintext.write_all(&buffer[..count])?;
    }
    plaintext.flush()?;
    drop(plaintext);
    drop(encrypted);

    let mut stored_tag = [0u8; TAG_BYTES];
    input
        .read_exact(&mut stored_tag)
        .map_err(|_| Failure::invalid("evidence.restore", "truncated encrypted evidence bundle"))?;
    let computed_tag = finish_gcm_tag(
        state,
        &mut tail,
        &mask,
        aad.len(),
        ciphertext_bytes,
        "evidence.restore",
    )?;
    if computed_tag.ct_eq(&stored_tag).unwrap_u8() != 1 {
        return Err(Failure::invalid(
            "evidence.restore",
            "encrypted evidence authentication failed: Unsupported state or unable to authenticate data",
        ));
    }
    Ok((temporary, ciphertext_bytes))
}

