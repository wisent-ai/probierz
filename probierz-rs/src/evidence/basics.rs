use crate::evidence::*;
use serde_json::json;
// The receipt verifier reads its receipt under this name; the reading is
// json_file::required, which names a missing or unreadable file.
pub(crate) use crate::json_file::required as json_file;
// A finite f64 as a JSON number (whole values as integers), else null: the
// encoding is run::number; receipt and bundle fields call it by this name.
pub(crate) use crate::run::number as js_number;

pub(crate) fn sha256_bytes(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

/// The SHA-256 of one file's bytes, streamed; every file hash in Probierz
/// (artifacts, bundles, adopted sources, builds) is read through this. A file
/// that cannot be opened or read is refused naming it.
pub(crate) fn sha256_file(file: &Path) -> Result<String, Failure> {
    let named = |action: &str, error: std::io::Error| {
        let failure = Failure::from(error);
        Failure::new(
            "evidence.hash",
            failure.code,
            format!("{action} {}: {}", file.display(), failure.detail),
        )
    };
    let mut input = File::open(file).map_err(|error| named("open", error))?;
    let mut digest = Sha256::new();
    std::io::copy(&mut input, &mut digest).map_err(|error| named("hash", error))?;
    Ok(hex::encode(digest.finalize()))
}

/// Canonical JSON: object keys sorted at every depth, compact. Signed
/// receipts, publications and the gate's receipt comparison all encode
/// through this; Value's Display is serde_json's compact encoding and has
/// no failure to answer with a made-up text.
pub(crate) fn canonical(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            let body = items.iter().map(canonical).collect::<Vec<_>>().join(",");
            format!("[{body}]")
        }
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            let body = keys
                .into_iter()
                .map(|key| format!("{}:{}", Value::from(key.as_str()), canonical(&object[key])))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
        primitive => primitive.to_string(),
    }
}

pub(crate) fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

/// Every run manifest below `root`. A directory that is not there holds
/// none, the root included; one that cannot be read, or an entry whose type
/// cannot be, is the failure naming it, never a run left out of the walk.
pub(crate) fn manifests_below(root: &Path) -> Result<Vec<PathBuf>, Failure> {
    let unreadable = |path: &Path, error: std::io::Error| {
        Failure::config(
            "evidence.runs",
            format!("run evidence {} cannot be read: {error}", path.display()),
        )
    };
    let mut pending = vec![root.to_path_buf()];
    let mut found = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(unreadable(&directory, error)),
        };
        for entry in entries {
            let entry = entry.map_err(|error| unreadable(&directory, error))?;
            let kind = entry
                .file_type()
                .map_err(|error| unreadable(&entry.path(), error))?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() && entry.file_name() == "run-manifest.json" {
                found.push(entry.path());
            }
        }
    }
    Ok(found)
}

pub(crate) fn normalized_status(manifest: &Value) -> String {
    match manifest.get("status").and_then(Value::as_str) {
        Some("passed" | "executed") => "passed",
        Some("blocked") => "blocked",
        Some("canceled") => "canceled",
        Some("failed") => "failed",
        _ if manifest
            .get("completedAt")
            .is_some_and(|value| !value.is_null()) =>
        {
            "failed"
        }
        _ => "incomplete",
    }
    .to_string()
}

/// The tests a run recorded, from its analysis or else its report: one per
/// title, in first-seen order, carrying the last record of each. A status or
/// duration the record did not state is null, never a failure or a zero, and
/// a record without a title is not a test. Only the analysis and report the
/// manifest names are read: a run that recorded none has none, and no file
/// is guessed beside the run. One that is named but cannot be read is the
/// failure naming it.
pub(crate) fn tests_from(
    _run_directory: &Path,
    manifest_value: &Value,
) -> Result<Vec<Value>, Failure> {
    let analysis = match manifest_value.get("analysisPath").and_then(Value::as_str) {
        Some(path) => crate::json_file::read(Path::new(path))?,
        None => None,
    };
    let report = match manifest_value.pointer("/paths/reportPath").and_then(Value::as_str) {
        Some(path) => crate::json_file::read(Path::new(path))?,
        None => None,
    };
    let source = analysis
        .as_ref()
        .and_then(|value| value.get("tests"))
        .and_then(Value::as_array)
        .or_else(|| {
            report
                .as_ref()
                .and_then(|value| value.get("tests"))
                .and_then(Value::as_array)
        });
    let mut order = Vec::<String>::new();
    let mut by_title = HashMap::<String, Value>::new();
    for test in source.into_iter().flatten() {
        let Some(title) = test.get("title").and_then(Value::as_str) else {
            continue;
        };
        if !by_title.contains_key(title) {
            order.push(title.to_string());
        }
        let status = match (
            test.get("status").and_then(Value::as_str),
            test.get("passed").and_then(Value::as_bool),
        ) {
            (Some(status), _) => json!(status),
            (None, Some(true)) => json!("passed"),
            (None, Some(false)) => json!("failed"),
            (None, None) => Value::Null,
        };
        let duration = match test
            .get("durationMs")
            .filter(|value| !value.is_null())
            .or_else(|| test.get("duration"))
            .and_then(Value::as_f64)
        {
            Some(duration) => crate::run::number(duration),
            None => Value::Null,
        };
        by_title.insert(
            title.to_string(),
            json!({ "title": title, "status": status, "durationMs": duration }),
        );
    }
    Ok(order
        .into_iter()
        .filter_map(|title| by_title.remove(&title))
        .collect())
}
