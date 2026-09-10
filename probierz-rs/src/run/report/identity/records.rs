use serde_json::json;
use crate::run::*;
pub(crate) fn write_json(file: &Path, value: &Value) -> Result<(), Failure> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = file.with_file_name(format!(
        "{}.tmp",
        file.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("value")
    ));
    let mut output = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode_600()
        .open(&temporary)?;
    writeln!(output, "{}", serde_json::to_string_pretty(value)?)?;
    fs::rename(temporary, file)?;
    Ok(())
}
pub(crate) trait Mode600 {
    fn mode_600(&mut self) -> &mut Self;
}
impl Mode600 for OpenOptions {
    fn mode_600(&mut self) -> &mut Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            self.mode(0o600);
        }
        self
    }
}

pub(crate) fn update_json(file: &Path, patch: &Value) -> Result<(), Failure> {
    let mut current: Value = fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| json!({}));
    if let (Some(target), Some(values)) = (current.as_object_mut(), patch.as_object()) {
        target.extend(values.clone());
    }
    write_json(file, &current)
}
pub(crate) fn artifact_hashes(directory: &Path, manifest_path: &Path) -> Result<Value, Failure> {
    let mut values = Vec::new();
    for file in walk(directory, true)
        .into_iter()
        .filter(|file| file != manifest_path)
    {
        values.push(json!({ "file": slash(file.strip_prefix(directory).unwrap_or(&file)), "sha256": sha256_file(&file)?, "bytes": fs::metadata(file)?.len() }));
    }
    Ok(Value::Array(values))
}
pub(crate) fn redacted_environment(values: &BTreeMap<String, String>) -> Value {
    Value::Object(
        values
            .iter()
            .map(|(name, value)| {
                let public = if sensitive_key(name) {
                    format!("[REDACTED:{name}]")
                } else {
                    value.clone()
                };
                (name.clone(), Value::String(public))
            })
            .collect(),
    )
}
pub(crate) fn run_conditions(record: bool, values: &BTreeMap<String, String>) -> Value {
    let mut conditions = Map::new();
    conditions.insert("record".into(), Value::Bool(record));
    for (name, value) in values {
        let public = if sensitive_key(name) {
            format!("[REDACTED:{name}]")
        } else {
            value.clone()
        };
        conditions.insert(name.clone(), Value::String(public));
    }
    Value::Object(conditions)
}
pub(crate) fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        architecture => architecture,
    }
}
pub(crate) fn secret_values(values: &BTreeMap<String, String>) -> Vec<(String, String)> {
    values
        .iter()
        .filter(|(name, value)| sensitive_key(name) && value.len() >= 4)
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}
pub(crate) fn redact_text(value: &str, secrets: &[(String, String)]) -> String {
    let mut safe = value.to_string();
    for (name, secret) in secrets {
        safe = safe.replace(secret, &format!("[REDACTED:{name}]"));
    }
    let expression = Regex::new(r"(?i)((?:AUTH|COOKIE|CREDENTIAL|EMAIL|GMAIL|KEY|OTP|PASSWORD|SECRET|SESSION|TOKEN)[A-Z0-9_]*\s*[=:]\s*)[^\s,;]+").expect("regex");
    safe = expression.replace_all(&safe, "$1[REDACTED]").into_owned();
    Regex::new(r#"(?i)("(?:auth|cookie|credential|email|gmail|key|otp|password|secret|session|token)[^"]*"\s*:\s*")[^"]*""#).expect("regex").replace_all(&safe, "$1[REDACTED]\"").into_owned()
}
pub(crate) fn stamped(value: &str) -> String {
    let stamp = now_iso();
    value
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{stamp} {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub(crate) fn tail_chars(value: &str, count: usize) -> String {
    let length = value.chars().count();
    value.chars().skip(length.saturating_sub(count)).collect()
}

pub(crate) fn app_surface(
    harness: &Path,
    app_id: &str,
    target: &str,
) -> Result<(manifest::Manifest, serde_yaml::Value), Failure> {
    let declaration = manifest::load(harness, app_id)?;
    let surface = declaration
        .document
        .get("surfaces")
        .and_then(|surfaces| surfaces.get(target))
        .cloned()
        .ok_or_else(|| {
            Failure::config("run.app", format!("app {app_id} has no {target} surface"))
        })?;
    Ok((declaration, surface))
}
pub(crate) fn yaml_map_strings(value: Option<&serde_yaml::Value>) -> BTreeMap<String, String> {
    value
        .and_then(serde_yaml::Value::as_mapping)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| Some((key.as_str()?.to_string(), yaml_string(value)?)))
                .collect()
        })
        .unwrap_or_default()
}
pub(crate) fn yaml_ordered_strings(value: Option<&serde_yaml::Value>) -> Map<String, Value> {
    let mut result = Map::new();
    if let Some(values) = value.and_then(serde_yaml::Value::as_mapping) {
        for (name, value) in values {
            if let (Some(name), Some(value)) = (name.as_str(), yaml_string(value)) {
                result.insert(name.into(), Value::String(value));
            }
        }
    }
    result
}

