use crate::gate::*;
/// A new Git ref has no old object: <https://git-scm.com/docs/githooks#_pre_push>.
pub(crate) const ZERO_SHA: &str = "0000000000000000000000000000000000000000";
pub(crate) const MANAGED_MARKER: &str = "# managed-by: probierz-prepush-gate";

/// Gate configuration wire version: <https://probierz.wisent.com/docs/cli/gate/activate>.
const GATE_CONFIG_SCHEMA_VERSION: u64 = 2;

#[derive(Debug, Clone, Args)]
pub struct GateArgs {
    pub app_id: String,
    pub mode: String,
    pub expected_harness_sha: String,
    #[arg(long = "source-sha")]
    pub expected_source_sha: Option<String>,
    #[arg(long)]
    pub runs: Option<String>,
    #[arg(long)]
    pub release: Option<String>,
    #[arg(long)]
    pub receipt: Option<PathBuf>,
    #[arg(long = "public-key")]
    pub public_key: Option<PathBuf>,
    #[arg(long)]
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct PrepushArgs {
    #[arg(long)]
    pub repo: Option<PathBuf>,
    #[arg(long = "app")]
    pub app_id: Option<String>,
    #[arg(long)]
    pub base: Option<String>,
    #[arg(long)]
    pub head: Option<String>,
    #[arg(long = "ci")]
    pub run_ci: bool,
    #[arg(long, hide = true)]
    pub hook: bool,
    #[arg(long, hide = true)]
    pub json: bool,
    #[arg(long = "ci-arg", hide = true)]
    pub ci_args: Vec<String>,
}

#[derive(Debug, Clone, Args)]
pub struct InstallArgs {
    pub app_id: String,
    #[arg(long)]
    pub repo: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct DeactivateArgs {
    /// Application whose gate mode is withdrawn.
    pub app_id: String,
    /// The mode to withdraw, as `gate activate` named it.
    pub mode: String,
    /// Why enforcement is withdrawn; recorded in the access audit.
    #[arg(long)]
    pub reason: String,
}

pub(crate) fn object(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in entries {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

pub(crate) fn strings(values: &[String]) -> Value {
    Value::Array(values.iter().cloned().map(Value::String).collect())
}

pub(crate) fn config_file(manifest: &manifest::Manifest) -> Result<PathBuf, Failure> {
    manifest
        .file
        .parent()
        .map(|directory| directory.join("gates.json"))
        .ok_or_else(|| {
            Failure::config(
                "gate.config",
                format!(
                    "manifest {} has no parent directory for gates.json",
                    manifest.file.display()
                ),
            )
        })
}

pub(crate) fn default_config(app_id: &str) -> Value {
    object([
        ("schemaVersion", Value::from(GATE_CONFIG_SCHEMA_VERSION)),
        ("appId", Value::String(app_id.to_string())),
        (
            "modes",
            object([
                (
                    "pull-request",
                    object([("enforcement", Value::String("pending-green".to_string()))]),
                ),
                (
                    "release",
                    object([("enforcement", Value::String("pending-green".to_string()))]),
                ),
            ]),
        ),
    ])
}

pub(crate) fn gate_status_value(harness: &Path, app_id: &str) -> Result<Value, Failure> {
    gate_config(&manifest::load(harness, app_id)?, app_id)
}

/// The gate config beside `app`'s manifest with its file and whether it
/// exists; an absent file is the default config. Gate commands and the status
/// report both read it here.
pub(crate) fn gate_config(app: &manifest::Manifest, app_id: &str) -> Result<Value, Failure> {
    let file = config_file(app)?;
    let recorded = crate::json_file::read(&file)?;
    let exists = recorded.is_some();
    let mut config = match recorded {
        Some(config) => config,
        None => default_config(app_id),
    };
    let map = config.as_object_mut().ok_or_else(|| {
        Failure::config(
            "gate.status",
            format!("{} does not contain a gate object", file.display()),
        )
    })?;
    map.insert(
        "file".to_string(),
        Value::String(file.to_string_lossy().into_owned()),
    );
    map.insert("exists".to_string(), Value::Bool(exists));
    Ok(config)
}

pub fn status(harness: &Path, app_id: &str) -> Answer {
    print_json(&gate_status_value(harness, app_id)?)
}

pub(crate) fn yaml_get<'a>(value: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    value.as_mapping()?.get(key)
}

pub(crate) fn yaml_string(value: Option<&Yaml>) -> Option<String> {
    value.and_then(Yaml::as_str).map(str::to_string)
}

/// An evidence level's rank is the number its name carries (`E<n>`); a name
/// that is not `E` followed by a number is not an evidence level. Every gate,
/// status, receipt and publication comparison reads ranks here.
pub(crate) fn evidence_rank(level: &str) -> Option<i32> {
    let digits = level.strip_prefix('E')?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    match digits.parse::<u8>() {
        Ok(rank) => Some(i32::from(rank)),
        Err(_out_of_range) => None,
    }
}

pub(crate) fn property<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.as_object()?.get(name)
}

pub(crate) fn string_property(value: &Value, name: &str) -> Option<String> {
    property(value, name)
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Required evidence lists never become empty lists because their shape is wrong.
pub(crate) fn value_array<'a>(
    value: Option<&'a Value>,
    location: &str,
) -> Result<&'a [Value], String> {
    match value {
        Some(Value::Array(list)) => Ok(list.as_slice()),
        Some(other) => Err(format!("{location} must be an array, found {other}")),
        None => Err(format!("{location} is missing; record an array")),
    }
}

pub(crate) fn value_strings(value: Option<&Value>, location: &str) -> Result<Vec<String>, String> {
    value_array(value, location)?
        .iter()
        .map(|value| match value {
            Value::String(text) if !text.trim().is_empty() => Ok(text.clone()),
            other => Err(format!(
                "{location} must contain non-empty text values, found {other}"
            )),
        })
        .collect()
}
