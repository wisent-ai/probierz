use serde_json::json;
use crate::run::*;
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

