use crate::evidence::*;
pub(crate) struct TemporaryPlaintext(PathBuf);

impl TemporaryPlaintext {
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryPlaintext {
    // The copy is decrypted evidence; one that stays on disk is said. A copy
    // already moved into place is gone, which is not a failure.
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.0) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "probierz: the decrypted evidence copy {} is left behind: {error}",
                    self.0.display()
                );
            }
        }
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
    let metadata = fs::metadata(file)?;
    let total_bytes = metadata.len();
    let overhead = (offset as u64)
        .checked_add(TAG_BYTES as u64)
        .ok_or_else(|| {
            Failure::invalid("evidence.restore", "truncated encrypted evidence bundle")
        })?;
    let ciphertext_bytes = total_bytes.checked_sub(overhead).ok_or_else(|| {
        Failure::invalid("evidence.restore", "truncated encrypted evidence bundle")
    })?;
    let (mut stream, mut state, mask) = gcm_state(key, nonce, aad, "evidence.restore")?;
    let mut tail = Vec::new();
    let directory = destination.parent().ok_or_else(|| {
        Failure::invalid(
            "evidence.restore",
            format!(
                "the restore destination {} names no directory to decrypt beside",
                destination.display()
            ),
        )
    })?;
    let temporary = TemporaryPlaintext(directory.join(format!(
        ".probierz-restore-{}-{}",
        std::process::id(),
        Utc::now().timestamp_millis(),
    )));
    let mut plaintext = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary.path())?;
    apply_mode(temporary.path(), u32::from(libc::S_IRUSR | libc::S_IWUSR))?;
    let mut input = File::open(file)?;
    input.seek(SeekFrom::Start(offset as u64))?;
    let mut encrypted = (&mut input).take(ciphertext_bytes);
    // One read per file-system block of the bundle, as the file system
    // reports it, so a large bundle streams without being held in memory.
    let block = std::os::unix::fs::MetadataExt::blksize(&metadata);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        (&mut encrypted).take(block).read_to_end(&mut buffer)?;
        if buffer.is_empty() {
            break;
        }
        ghash_feed(&mut state, &mut tail, &buffer);
        stream.try_apply_keystream(&mut buffer).map_err(|_| {
            Failure::invalid("evidence.restore", "encrypted evidence payload is too large")
        })?;
        plaintext.write_all(&buffer)?;
    }
    plaintext.flush()?;
    drop(plaintext);
    drop(encrypted);

    let mut stored_tag = Vec::new();
    (&mut input).take(TAG_BYTES as u64).read_to_end(&mut stored_tag)?;
    if stored_tag.len() != TAG_BYTES {
        return Err(Failure::invalid("evidence.restore", "truncated encrypted evidence bundle"));
    }
    let computed_tag = finish_gcm_tag(
        state,
        &mut tail,
        &mask,
        aad.len(),
        ciphertext_bytes,
        "evidence.restore",
    )?;
    if !bool::from(computed_tag.as_slice().ct_eq(stored_tag.as_slice())) {
        return Err(Failure::invalid(
            "evidence.restore",
            "encrypted evidence authentication failed: Unsupported state or unable to authenticate data",
        ));
    }
    Ok((temporary, ciphertext_bytes))
}
