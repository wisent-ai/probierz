use crate::run::*;
use crate::run::{DateTime, Utc};
pub(crate) fn segment(value: Option<&str>, fallback: &str) -> String {
    let source = value.unwrap_or(fallback).trim();
    let mut result = String::new();
    let mut dash = false;
    for ch in source.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            result.push(ch);
            dash = false;
        } else if !dash {
            result.push('-');
            dash = true;
        }
    }
    if result.is_empty() {
        fallback.into()
    } else {
        result
    }
}
pub(crate) fn unique_run_id(started: DateTime<Utc>) -> String {
    let seed = format!(
        "{}:{}:{}",
        started.timestamp_nanos_opt().unwrap_or_default(),
        std::process::id(),
        std::thread::current().name().unwrap_or("main")
    );
    let digest = Sha256::digest(seed.as_bytes());
    let uuid = format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        u32::from_be_bytes(digest[0..4].try_into().expect("slice")),
        u16::from_be_bytes(digest[4..6].try_into().expect("slice")),
        u16::from_be_bytes(digest[6..8].try_into().expect("slice")) & 0x0fff,
        (u16::from_be_bytes(digest[8..10].try_into().expect("slice")) & 0x3fff) | 0x8000,
        u64::from_be_bytes(digest[10..18].try_into().expect("slice")) & 0x0000ffffffffffff
    );
    format!(
        "{}-{uuid}",
        started
            .to_rfc3339_opts(SecondsFormat::Millis, true)
            .replace([':', '.'], "-")
    )
}
pub(crate) fn sha256_file(file: &Path) -> Result<String, Failure> {
    let mut source = File::open(file)?;
    let mut hash = Sha256::new();
    std::io::copy(&mut source, &mut hash).map_err(Failure::from)?;
    Ok(hex::encode(hash.finalize()))
}
