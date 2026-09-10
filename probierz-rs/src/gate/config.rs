use crate::gate::*;
pub(crate) const ZERO_SHA: &str = "0000000000000000000000000000000000000000";
pub(crate) const MANAGED_MARKER: &str = "# managed-by: probierz-prepush-gate";

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

pub(crate) fn config_file(manifest: &manifest::Manifest) -> PathBuf {
    manifest
        .file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("gates.json")
}

pub(crate) fn default_config(app_id: &str) -> Value {
    object([
        ("schemaVersion", Value::from(2)),
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
    let app = manifest::load(harness, app_id)?;
    let file = config_file(&app);
    let exists = file.exists();
    let mut config = if exists {
        serde_json::from_str::<Value>(&fs::read_to_string(&file)?)?
    } else {
        default_config(app_id)
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
    value.as_mapping()?.get(&Yaml::String(key.to_string()))
}

pub(crate) fn yaml_string(value: Option<&Yaml>) -> Option<String> {
    value.and_then(Yaml::as_str).map(str::to_string)
}

pub(crate) fn yaml_strings(value: Option<&Yaml>) -> Vec<String> {
    value
        .and_then(Yaml::as_sequence)
        .map(|list| {
            list.iter()
                .filter_map(Yaml::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn yaml_bool(value: Option<&Yaml>) -> bool {
    value.and_then(Yaml::as_bool).unwrap_or(false)
}

pub(crate) fn yaml_js_string(value: &Yaml) -> String {
    match value {
        Yaml::Null => "null".to_string(),
        Yaml::Bool(flag) => flag.to_string(),
        Yaml::Number(number) => number.to_string(),
        Yaml::String(text) => text.clone(),
        other => serde_json::to_string(&serde_json::to_value(other).unwrap_or(Value::Null))
            .unwrap_or_default(),
    }
}

pub(crate) fn evidence_rank(level: &str) -> Option<i32> {
    match level {
        "E0" => Some(0),
        "E1" => Some(1),
        "E2" => Some(2),
        "E3" => Some(3),
        _ => None,
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

pub(crate) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number
            .as_f64()
            .map(|number| number != 0.0 && !number.is_nan())
            .unwrap_or(false),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

pub(crate) fn value_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

pub(crate) fn value_strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

