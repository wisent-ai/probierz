use crate::adoption::*;
pub(crate) fn target_package(target: &str) -> Option<&'static str> {
    TARGET_PACKAGES
        .iter()
        .find_map(|(known, package)| (*known == target).then_some(*package))
}

pub(crate) fn matches_declared_spec(pattern: &str, relative_to_package: &str) -> bool {
    let trimmed = pattern.strip_prefix("./").unwrap_or(pattern);
    let normalized_storage;
    let normalized = if std::path::MAIN_SEPARATOR == '\\' {
        normalized_storage = trimmed.replace('\\', "/");
        normalized_storage.as_str()
    } else {
        trimmed
    };
    let compared = if normalized.contains('/') {
        relative_to_package
    } else {
        relative_to_package
            .rsplit('/')
            .next()
            .unwrap_or(relative_to_package)
    };
    wildcard_match(normalized, compared)
}

pub(crate) fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    let pattern = pattern.as_bytes();
    let candidate = candidate.as_bytes();
    let (mut pattern_index, mut candidate_index) = (0usize, 0usize);
    let (mut star, mut retry_candidate) = (None, 0usize);
    while candidate_index < candidate.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == candidate[candidate_index] {
            pattern_index += 1;
            candidate_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star = Some(pattern_index);
            pattern_index += 1;
            retry_candidate = candidate_index;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            retry_candidate += 1;
            candidate_index = retry_candidate;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

pub(crate) fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<(), Failure> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(unix)]
pub(crate) fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o777
}

#[cfg(not(unix))]
pub(crate) fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    if metadata.permissions().readonly() {
        0o444
    } else {
        0o666
    }
}

#[cfg(unix)]
pub(crate) fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
pub(crate) fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(mode & 0o200 == 0);
    fs::set_permissions(path, permissions)
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, Failure> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

pub(crate) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(crate) fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(crate) fn os_text(value: &OsStr) -> &str {
    value.to_str().unwrap_or_default()
}

