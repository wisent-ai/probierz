//! Run records: the manifests below a root, their failure class, and the projections status reports print. The record one run manifest becomes is `crate::evidence::run_record`.

use crate::status::*;

pub(crate) fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn json_number(value: f64) -> Value {
    if value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value <= i64::MAX as f64
    {
        Value::Number(Number::from(value as i64))
    } else {
        Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

pub(crate) fn number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(value)) => value.as_f64().unwrap_or(0.0),
        Some(Value::String(value)) => value.parse::<f64>().unwrap_or(f64::NAN),
        Some(Value::Bool(value)) => usize::from(*value) as f64,
        Some(Value::Null) | None => 0.0,
        _ => f64::NAN,
    }
}

pub(crate) fn string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

/// A report field as printed: its text, count or yes/no, or the field named
/// as missing — never a zero, an empty word or a no in its place. Every text
/// renderer of a status report prints its fields through this.
pub(crate) fn said(value: Option<&Value>, name: &str) -> String {
    match value {
        Some(Value::String(text)) if !text.is_empty() => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        _ => format!("(no {name})"),
    }
}

pub(crate) fn manifests_below(root: &Path) -> Result<Vec<PathBuf>, Failure> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() && entry.file_name() == "run-manifest.json" {
                files.push(entry.path());
            }
        }
    }
    Ok(files)
}

/// Projects recorded ownership for status, history and evidence consumers.
pub(crate) fn failure_class(manifest: &Value) -> &'static str {
    match manifest.get("failureOrigin") {
        Some(Value::String(origin)) if origin == "harness" => "infrastructure",
        Some(Value::String(origin)) if origin == "product" => "product",
        None if string(manifest.get("setupError")).is_some() => "infrastructure",
        _ => "unclassified",
    }
}

pub(crate) fn value_or(value: Option<&Value>, fallback: Value) -> Value {
    match value {
        Some(Value::Null) | None => fallback,
        Some(value) => value.clone(),
    }
}
