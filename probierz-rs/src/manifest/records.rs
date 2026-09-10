use crate::manifest::*;
/// Keys that must never carry a value in a manifest: they belong in
/// `secretRefs`, which resolves through the vault instead.
pub(crate) const SENSITIVE: [&str; 10] = [
    "auth",
    "cookie",
    "credential",
    "email",
    "key",
    "otp",
    "password",
    "pii",
    "secret",
    "session",
];

pub(crate) const PUBLICATION_ARTIFACT_KINDS: [&str; 3] = ["screenshot", "recording", "trace"];

/// The targets whose driver can record a screen. A journey that claims a
/// recording on a driver that cannot make one is refused.
pub(crate) const RECORDING_TARGETS: [&str; 6] = [
    "web",
    "mobile:ios",
    "mobile:android",
    "desktop:mac",
    "desktop:cua",
    "desktop:win",
];

pub fn target_supports_artifact_kind(target: &str, kind: &str) -> bool {
    if !PUBLICATION_ARTIFACT_KINDS.contains(&kind) {
        return false;
    }
    kind != "recording" || RECORDING_TARGETS.contains(&target)
}

/// One validated manifest, with the file it was read from.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub app_id: String,
    pub file: PathBuf,
    pub document: Value,
}

/// What `probierz apps` answers with, per product.
#[derive(Debug, Clone, Serialize)]
pub struct AppSummary {
    #[serde(rename = "appId")]
    pub app_id: String,
    pub owner: String,
    pub file: String,
    pub targets: Vec<String>,
    pub journeys: Vec<String>,
}

pub fn apps_root(harness_root: &Path) -> PathBuf {
    harness_root.join("apps")
}

pub(crate) fn sensitive(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    SENSITIVE.iter().any(|needle| lowered.contains(needle))
}

pub(crate) fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        let expected_dash = matches!(index, 8 | 13 | 18 | 23);
        if expected_dash {
            if *byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    matches!(bytes[14], b'1'..=b'5') && matches!(bytes[19] | 0x20, b'8' | b'9' | b'a' | b'b')
}

pub(crate) fn valid_id(value: &str) -> bool {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
}

pub(crate) fn require(condition: bool, file: &Path, what: &str) -> Answer {
    if condition {
        Ok(())
    } else {
        Err(Failure::new(
            "manifest.validate",
            Code::Config,
            format!("invalid app manifest: {} {what}", file.display()),
        ))
    }
}

pub(crate) fn map_of<'a>(value: &'a Value, key: &str) -> Option<&'a serde_yaml::Mapping> {
    value.get(key).and_then(Value::as_mapping)
}

pub(crate) fn string_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

pub(crate) fn sequence_of<'a>(value: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    value.get(key).and_then(Value::as_sequence)
}

pub(crate) fn key_names(mapping: &serde_yaml::Mapping) -> Vec<String> {
    mapping
        .keys()
        .filter_map(|key| key.as_str().map(str::to_string))
        .collect()
}

