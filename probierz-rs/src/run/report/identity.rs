use serde_json::json;
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

pub(crate) fn repository_identity(
    root: &Path,
    name: &str,
    index: Option<usize>,
    exclude_secrets: bool,
    package_lock: bool,
) -> Result<Value, Failure> {
    let files = git_source_paths(root, exclude_secrets, package_lock)?;
    let mut hash = Sha256::new();
    for relative in files {
        let file = root.join(&relative);
        let meta = fs::symlink_metadata(&file)?;
        let (kind, payload) = if meta.file_type().is_symlink() {
            (
                "symlink",
                fs::read_link(&file)?.to_string_lossy().as_bytes().to_vec(),
            )
        } else {
            ("file", fs::read(&file)?)
        };
        let header = json!({ "path": relative, "kind": kind, "mode": file_mode(&meta), "bytes": payload.len() }).to_string();
        hash.update(format!("{}:", header.len()).as_bytes());
        hash.update(header.as_bytes());
        hash.update(payload);
    }
    let worktree = hex::encode(hash.finalize());
    let head = capture(
        "git",
        &[
            "-C".into(),
            root.to_string_lossy().into_owned(),
            "rev-parse".into(),
            "HEAD".into(),
        ],
        None,
        None,
        None,
    );
    let diff = capture(
        "git",
        &[
            "-C".into(),
            root.to_string_lossy().into_owned(),
            "diff".into(),
            "--quiet".into(),
            "HEAD".into(),
            "--".into(),
        ],
        None,
        None,
        None,
    );
    let others = capture(
        "git",
        &[
            "-C".into(),
            root.to_string_lossy().into_owned(),
            "ls-files".into(),
            "--others".into(),
            "--exclude-standard".into(),
            "-z".into(),
        ],
        None,
        None,
        None,
    );
    let mut exact = Map::new();
    if let Some(index) = index {
        exact.insert("index".into(), json!(index));
    }
    exact.insert("name".into(), json!(name));
    exact.insert("worktreeSha256".into(), json!(worktree));
    let sha = hex::encode(Sha256::digest(
        Value::Object(exact.clone()).to_string().as_bytes(),
    ));
    let mut result = Map::new();
    if let Some(index) = index {
        result.insert("index".into(), json!(index));
    }
    result.insert("name".into(), json!(name));
    result.insert(
        "gitSha".into(),
        if head.status.is_some_and(|status| status.success()) {
            json!(text(&head.stdout).trim())
        } else {
            Value::Null
        },
    );
    result.insert(
        "dirty".into(),
        json!(!diff.status.is_some_and(|status| status.success()) || !others.stdout.is_empty()),
    );
    result.insert("worktreeSha256".into(), json!(worktree));
    result.insert("sha256".into(), json!(sha));
    Ok(Value::Object(result))
}

pub(crate) fn submitted_source_identity(app_id: Option<&str>) -> Result<Option<Value>, Failure> {
    let Some(file) = std::env::var_os("PROBIERZ_SOURCE_IDENTITY").map(PathBuf::from) else {
        return Ok(None);
    };
    let document: Value = serde_json::from_slice(&fs::read(&file)?)
        .map_err(|error| Failure::config("run.source", format!("{}: {error}", file.display())))?;
    let valid_hash = |value: Option<&str>| {
        value.is_some_and(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
    };
    if document.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || !valid_hash(
            document
                .pointer("/harness/worktreeSha256")
                .and_then(Value::as_str),
        )
    {
        return Err(Failure::config(
            "run.source",
            format!("{}: unusable source identity", file.display()),
        ));
    }
    for repository in document
        .pointer("/app/repositories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !valid_hash(repository.get("worktreeSha256").and_then(Value::as_str)) {
            return Err(Failure::config(
                "run.source",
                format!("{}: repository has no worktreeSha256", file.display()),
            ));
        }
    }
    if app_id.is_some()
        && document.get("appId").and_then(Value::as_str).is_some()
        && document.get("appId").and_then(Value::as_str) != app_id
    {
        return Ok(None);
    }
    Ok(Some(document))
}

pub fn app_source_identity(harness: &Path, app_id: &str) -> Result<Value, Failure> {
    if let Some(submitted) = submitted_source_identity(Some(app_id))? {
        return Ok(submitted);
    }
    let declaration = manifest::load(harness, app_id)?;
    let repositories = declaration
        .document
        .get("repositories")
        .and_then(serde_yaml::Value::as_sequence)
        .cloned()
        .unwrap_or_default();
    let mut identities = Vec::new();
    for (index, repository) in repositories.iter().enumerate() {
        let root = repository
            .get("root")
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or("");
        identities.push(repository_identity(
            Path::new(root),
            Path::new(root)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
            Some(index),
            false,
            false,
        )?);
    }
    let exact: Vec<Value> = identities
        .iter()
        .map(|identity| json!({ "index": identity["index"], "sha256": identity["sha256"] }))
        .collect();
    let app = json!({ "sha256": hex::encode(Sha256::digest(Value::Array(exact).to_string().as_bytes())), "repositories": identities });
    Ok(
        json!({ "schemaVersion": 1, "appId": app_id, "harness": repository_identity(harness, "probierz", None, true, true)?, "app": app }),
    )
}

pub(crate) fn build_identity(harness: &Path, env: &BTreeMap<String, String>) -> Result<Value, Failure> {
    let candidate = [
        "PROBIERZ_BUILD_PATH",
        "APP_IOS",
        "MAC_APP_PATH",
        "ELECTRON_APP_MAIN",
    ]
    .iter()
    .find_map(|name| env.get(*name).cloned())
    .unwrap_or_else(|| {
        harness
            .join("package-lock.json")
            .to_string_lossy()
            .into_owned()
    });
    let candidate_path = Path::new(&candidate);
    let resolved = if candidate_path.is_absolute() {
        candidate_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(candidate_path)
    };
    let path = normalize_path(&resolved);
    let sha = if path.is_file() {
        Some(sha256_file(&path)?)
    } else if path.is_dir() {
        let mut hash = Sha256::new();
        for file in walk(&path, true) {
            hash.update(
                file.strip_prefix(&path)
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .as_bytes(),
            );
            hash.update(fs::read(file)?);
        }
        Some(hex::encode(hash.finalize()))
    } else {
        None
    };
    Ok(json!({ "path": path, "sha256": sha }))
}

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

