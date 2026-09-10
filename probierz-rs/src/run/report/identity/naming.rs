use crate::run::{DateTime, Utc};
use crate::run::*;
pub(crate) fn sensitive_key(name: &str) -> bool {
    [
        "auth",
        "cookie",
        "credential",
        "email",
        "gmail",
        "key",
        "otp",
        "password",
        "pii",
        "secret",
        "session",
        "token",
    ]
    .iter()
    .any(|part| name.to_ascii_lowercase().contains(part))
}
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

#[cfg(unix)]
pub(crate) fn file_mode(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.mode() & 0o777
}
#[cfg(not(unix))]
pub(crate) fn file_mode(_meta: &fs::Metadata) -> u32 {
    0
}

pub(crate) fn git_source_paths(
    root: &Path,
    exclude_secrets: bool,
    package_lock: bool,
) -> Result<Vec<String>, Failure> {
    let result = capture(
        "git",
        &[
            "-C".into(),
            root.to_string_lossy().into_owned(),
            "ls-files".into(),
            "--cached".into(),
            "--others".into(),
            "--exclude-standard".into(),
            "-z".into(),
        ],
        None,
        None,
        None,
    );
    if !result.status.is_some_and(|status| status.success()) {
        return Err(Failure::config(
            "run.source",
            format!(
                "git ls-files in {}: {}",
                root.display(),
                text(&result.stderr).trim()
            ),
        ));
    }
    let mut paths: BTreeSet<String> = text(&result.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .filter(|relative| {
            let parts: Vec<&str> = relative.split('/').collect();
            !Path::new(relative).is_absolute()
                && !parts.contains(&"..")
                && !parts
                    .iter()
                    .any(|part| matches!(*part, "node_modules" | "test-results"))
                && (!exclude_secrets
                    || (!parts.iter().any(|part| part.starts_with(".env"))
                        && !(relative
                            .rsplit('/')
                            .next()
                            .unwrap_or("")
                            .starts_with("probierz-")
                            && relative.ends_with(".json"))))
        })
        .filter(|relative| {
            fs::symlink_metadata(root.join(relative))
                .is_ok_and(|meta| meta.is_file() || meta.file_type().is_symlink())
        })
        .map(str::to_string)
        .collect();
    if package_lock && root.join("package-lock.json").exists() {
        paths.insert("package-lock.json".into());
    }
    Ok(paths.into_iter().collect())
}

