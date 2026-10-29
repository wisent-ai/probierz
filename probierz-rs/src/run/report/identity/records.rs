use crate::run::*;
use serde_json::json;
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

pub(crate) fn artifact_hashes(directory: &Path, manifest_path: &Path) -> Result<Value, Failure> {
    let mut values = Vec::new();
    for file in walk(directory, true)
        .into_iter()
        .filter(|file| file != manifest_path)
    {
        // walk lists files below the run's directory; one outside it is
        // refused, never recorded under its absolute path.
        let relative = file.strip_prefix(directory).map_err(|error| {
            Failure::config(
                "run.artifacts",
                format!(
                    "{} is not under {}: {error}",
                    file.display(),
                    directory.display()
                ),
            )
        })?;
        values.push(json!({ "file": slash(relative), "sha256": crate::evidence::sha256_file(&file)?, "bytes": fs::metadata(&file)?.len() }));
    }
    Ok(Value::Array(values))
}
/// The conditions a run records: whether it records, and every condition
/// with each value its redaction hides replaced by its name.
pub(crate) fn run_conditions(
    record: bool,
    values: &BTreeMap<String, String>,
    redaction: &manifest::Redaction,
) -> Value {
    let mut conditions = Map::new();
    conditions.insert("record".into(), Value::Bool(record));
    if let Value::Object(named) = redaction.environment(values) {
        conditions.extend(named);
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
                .filter_map(|(key, value)| {
                    Some((key.as_str()?.to_string(), yaml_scalar_text(value)?))
                })
                .collect()
        })
        .unwrap_or_default()
}
pub(crate) fn yaml_ordered_strings(value: Option<&serde_yaml::Value>) -> Map<String, Value> {
    let mut result = Map::new();
    if let Some(values) = value.and_then(serde_yaml::Value::as_mapping) {
        for (name, value) in values {
            if let (Some(name), Some(value)) = (name.as_str(), yaml_scalar_text(value)) {
                result.insert(name.into(), Value::String(value));
            }
        }
    }
    result
}
