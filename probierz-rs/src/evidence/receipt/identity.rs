use crate::evidence::*;
use serde_json::json;

mod keys;
pub(crate) use keys::*;
pub(crate) fn git_output(root: &Path, args: &[&str]) -> Result<std::process::Output, Failure> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| Failure::config("run.source", error.to_string()))
}

pub(crate) fn repository_source_files(
    root: &Path,
    exclude_runtime_secrets: bool,
    include_package_lock: bool,
) -> Result<Vec<String>, Failure> {
    let output = git_output(
        root,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?;
    if !output.status.success() {
        return Err(Failure::config(
            "run.source",
            format!(
                "git ls-files in {}: {}",
                root.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    let mut files = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
        .filter_map(|item| String::from_utf8(item.to_vec()).ok())
        .filter(|relative| {
            let parts = relative.split('/').collect::<Vec<_>>();
            !Path::new(relative).is_absolute()
                && !parts.contains(&"..")
                && !parts
                    .iter()
                    .any(|part| matches!(*part, "node_modules" | "test-results"))
                && (!exclude_runtime_secrets
                    || (!parts.iter().any(|part| part.starts_with(".env"))
                        && !(relative
                            .rsplit('/')
                            .next()
                            .unwrap_or(relative)
                            .starts_with("probierz-")
                            && relative.ends_with(".json"))))
        })
        .filter(|relative| {
            fs::symlink_metadata(root.join(relative))
                .is_ok_and(|metadata| metadata.is_file() || metadata.file_type().is_symlink())
        })
        .collect::<BTreeSet<_>>();
    if include_package_lock && root.join("package-lock.json").exists() {
        files.insert("package-lock.json".into());
    }
    Ok(files.into_iter().collect())
}

pub(crate) fn repository_identity(
    root: &Path,
    name: &str,
    index: Option<usize>,
    exclude_runtime_secrets: bool,
    include_package_lock: bool,
) -> Result<Value, Failure> {
    let head = git_output(root, &["rev-parse", "HEAD"])?;
    let diff = git_output(root, &["diff", "--quiet", "HEAD", "--"])?;
    let others = git_output(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    let files = repository_source_files(root, exclude_runtime_secrets, include_package_lock)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 128 * 1024];
    for relative in files {
        let file = root.join(&relative);
        let metadata = fs::symlink_metadata(&file)?;
        let symlink = if metadata.file_type().is_symlink() {
            Some(
                fs::read_link(&file)?
                    .to_string_lossy()
                    .into_owned()
                    .into_bytes(),
            )
        } else {
            None
        };
        let header = json!({
            "path": relative,
            "kind": if symlink.is_some() { "symlink" } else { "file" },
            "mode": file_mode(&metadata),
            "bytes": symlink.as_ref().map_or(metadata.len(), |payload| payload.len() as u64),
        });
        let encoded = serde_json::to_string(&header)?;
        digest.update(format!("{}:", encoded.len()).as_bytes());
        digest.update(encoded.as_bytes());
        if let Some(payload) = symlink {
            digest.update(payload);
        } else {
            let mut input = File::open(&file)?;
            loop {
                let count = input.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                digest.update(&buffer[..count]);
            }
        }
    }
    let worktree = hex::encode(digest.finalize());
    let git_sha = if head.status.success() {
        Value::String(String::from_utf8_lossy(&head.stdout).trim().to_string())
    } else {
        Value::Null
    };
    let dirty = !diff.status.success() || !others.stdout.is_empty();
    let mut identity = Map::new();
    if let Some(index) = index {
        identity.insert("index".into(), json!(index));
    }
    identity.insert("name".into(), json!(name));
    identity.insert("gitSha".into(), git_sha);
    identity.insert("dirty".into(), json!(dirty));
    identity.insert("worktreeSha256".into(), json!(worktree));
    let exact = if let Some(index) = index {
        json!({ "index": index, "name": name, "worktreeSha256": worktree })
    } else {
        json!({ "name": name, "worktreeSha256": worktree })
    };
    identity.insert(
        "sha256".into(),
        json!(sha256_bytes(serde_json::to_string(&exact)?.as_bytes())),
    );
    Ok(Value::Object(identity))
}

pub(crate) fn app_source_identity(harness: &Path, app_id: &str) -> Result<Value, Failure> {
    if let Some(file) = std::env::var_os("PROBIERZ_SOURCE_IDENTITY") {
        let parsed = json_file(Path::new(&file))?;
        if parsed.get("schemaVersion").and_then(Value::as_i64) != Some(1) {
            return Err(Failure::config(
                "run.source",
                format!("{}: schemaVersion must be 1", Path::new(&file).display()),
            ));
        }
        if parsed
            .pointer("/harness/worktreeSha256")
            .and_then(Value::as_str)
            .is_none_or(|value| !is_sha256(value))
        {
            return Err(Failure::config(
                "run.source",
                format!(
                    "{}: harness worktreeSha256 is missing",
                    Path::new(&file).display()
                ),
            ));
        }
        for repository in parsed
            .pointer("/app/repositories")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if repository
                .get("worktreeSha256")
                .and_then(Value::as_str)
                .is_none_or(|value| !is_sha256(value))
            {
                let name = repository
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                return Err(Failure::config(
                    "run.source",
                    format!(
                        "{}: repository {name} has no worktreeSha256",
                        Path::new(&file).display()
                    ),
                ));
            }
        }
        if parsed
            .get("appId")
            .and_then(Value::as_str)
            .is_none_or(|declared| declared == app_id)
        {
            return Ok(parsed);
        }
    }
    let loaded = manifest::load(harness, app_id)?;
    let document = yaml_json(&loaded.document)?;
    let mut repositories = Vec::new();
    for (index, repository) in document
        .get("repositories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let root = PathBuf::from(
            repository
                .get("root")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        let name = root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        repositories.push(repository_identity(&root, name, Some(index), false, false)?);
    }
    let app_exact = Value::Array(repositories.iter().map(|repo| json!({
        "index": repo.get("index").cloned().unwrap_or(Value::Null), "sha256": repo.get("sha256").cloned().unwrap_or(Value::Null),
    })).collect());
    let app = json!({ "sha256": sha256_bytes(serde_json::to_string(&app_exact)?.as_bytes()), "repositories": repositories });
    Ok(json!({
        "schemaVersion": 1, "appId": app_id,
        "harness": repository_identity(harness, "probierz", None, true, true)?,
        "app": app,
    }))
}
pub(crate) fn app_source_identity_value(harness: &Path, app_id: &str) -> Result<Value, Failure> {
    app_source_identity(harness, app_id)
}

pub(crate) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
pub(crate) fn is_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
