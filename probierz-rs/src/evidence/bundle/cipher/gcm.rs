use crate::evidence::*;

/// Feed bytes into the running GHASH, keeping any partial block for the
/// next call.
pub(crate) fn ghash_feed(state: &mut GHash, tail: &mut Vec<u8>, mut bytes: &[u8]) {
    if !tail.is_empty() {
        let needed = 16 - tail.len();
        let take = needed.min(bytes.len());
        tail.extend_from_slice(&bytes[..take]);
        bytes = &bytes[take..];
        if tail.len() == 16 {
            let block = *ghash::Block::from_slice(tail);
            state.update(&[block]);
            tail.clear();
        }
    }
    while bytes.len() >= 16 {
        let block = *ghash::Block::from_slice(&bytes[..16]);
        state.update(&[block]);
        bytes = &bytes[16..];
    }
    tail.extend_from_slice(bytes);
}

pub(crate) fn ghash_pad(state: &mut GHash, tail: &mut Vec<u8>) {
    if tail.is_empty() {
        return;
    }
    tail.resize(16, 0);
    let block = *ghash::Block::from_slice(tail);
    state.update(&[block]);
    tail.clear();
}

pub(crate) fn gcm_state(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    point: &'static str,
) -> Result<(Aes256Ctr, GHash, [u8; 16]), Failure> {
    if nonce.len() != 12 {
        return Err(Failure::invalid(
            point,
            "encrypted evidence nonce is invalid",
        ));
    }
    let aes =
        Aes256::new_from_slice(key).map_err(|error| Failure::config(point, error.to_string()))?;
    let mut hash_key = aes::cipher::Block::<Aes256>::default();
    aes.encrypt_block(&mut hash_key);
    let mut state = GHash::new(ghash::Key::from_slice(&hash_key));
    let mut aad_tail = Vec::with_capacity(16);
    ghash_feed(&mut state, &mut aad_tail, aad);
    ghash_pad(&mut state, &mut aad_tail);

    let mut initial_counter = [0u8; 16];
    initial_counter[..12].copy_from_slice(nonce);
    initial_counter[15] = 2;
    let stream = Aes256Ctr::new_from_slices(key, &initial_counter)
        .map_err(|error| Failure::config(point, error.to_string()))?;

    initial_counter[15] = 1;
    let mut tag_mask = aes::cipher::Block::<Aes256>::clone_from_slice(&initial_counter);
    aes.encrypt_block(&mut tag_mask);
    let mut mask = [0u8; 16];
    mask.copy_from_slice(&tag_mask);
    Ok((stream, state, mask))
}

pub(crate) fn finish_gcm_tag(
    mut state: GHash,
    tail: &mut Vec<u8>,
    mask: &[u8; 16],
    aad_bytes: usize,
    ciphertext_bytes: u64,
    point: &'static str,
) -> Result<[u8; 16], Failure> {
    ghash_pad(&mut state, tail);
    let aad_bits = u64::try_from(aad_bytes)
        .ok()
        .and_then(|value| value.checked_mul(8))
        .ok_or_else(|| Failure::invalid(point, "encrypted evidence header is too large"))?;
    let ciphertext_bits = ciphertext_bytes
        .checked_mul(8)
        .ok_or_else(|| Failure::invalid(point, "encrypted evidence payload is too large"))?;
    let mut lengths = [0u8; 16];
    lengths[..8].copy_from_slice(&aad_bits.to_be_bytes());
    lengths[8..].copy_from_slice(&ciphertext_bits.to_be_bytes());
    state.update(&[*ghash::Block::from_slice(&lengths)]);
    let mut tag = state.finalize();
    for (byte, mask_byte) in tag.iter_mut().zip(mask) {
        *byte ^= mask_byte;
    }
    let mut result = [0u8; 16];
    result.copy_from_slice(&tag);
    Ok(result)
}

pub(crate) fn encrypt_chunk(
    output: &mut File,
    stream: &mut Aes256Ctr,
    state: &mut GHash,
    tail: &mut Vec<u8>,
    bytes: &mut [u8],
) -> Result<(), Failure> {
    stream.try_apply_keystream(bytes).map_err(|_| {
        Failure::invalid(
            "evidence.protect",
            "encrypted evidence payload is too large",
        )
    })?;
    ghash_feed(state, tail, bytes);
    output.write_all(bytes)?;
    Ok(())
}

pub(crate) fn encrypt_bundle_payload(
    output: &mut File,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    index: &[u8],
    source_files: &[PathBuf],
) -> Result<[u8; 16], Failure> {
    let (mut stream, mut state, mask) = gcm_state(key, nonce, aad, "evidence.protect")?;
    let mut tail = Vec::with_capacity(16);
    let mut prefix = Vec::with_capacity(4 + index.len());
    prefix.extend_from_slice(&(index.len() as u32).to_be_bytes());
    prefix.extend_from_slice(index);
    let mut ciphertext_bytes = prefix.len() as u64;
    encrypt_chunk(output, &mut stream, &mut state, &mut tail, &mut prefix)?;

    let mut buffer = vec![0u8; 128 * 1024];
    for file in source_files {
        let mut input = File::open(file)?;
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            ciphertext_bytes = ciphertext_bytes.checked_add(count as u64).ok_or_else(|| {
                Failure::invalid(
                    "evidence.protect",
                    "encrypted evidence payload is too large",
                )
            })?;
            encrypt_chunk(
                output,
                &mut stream,
                &mut state,
                &mut tail,
                &mut buffer[..count],
            )?;
        }
    }
    finish_gcm_tag(
        state,
        &mut tail,
        &mask,
        aad.len(),
        ciphertext_bytes,
        "evidence.protect",
    )
}

