use crate::authoring::*;
use serde_json::json;

/// The schema of a source identity document, the one `app_source_identity`
/// writes and `submitted_source_identity` reads.
const IDENTITY_SCHEMA_VERSION: u64 = 1;
pub(crate) fn repository_identity(
    root: &Path,
    name: &str,
    index: Option<usize>,
    exclude_runtime_secrets: bool,
    include_package_lock: bool,
) -> Result<JsonValue, Failure> {
    let head = command_output(
        "git",
        &["-C", &root.to_string_lossy(), "rev-parse", "HEAD"],
        None,
    )?;
    let diff = command_output(
        "git",
        &[
            "-C",
            &root.to_string_lossy(),
            "diff",
            "--quiet",
            "HEAD",
            "--",
        ],
        None,
    )?;
    let others = git_paths(root, true)?;
    let files = repository_source_files(root, exclude_runtime_secrets, include_package_lock)?;
    let worktree_sha = hash_source_files(root, &files)?;

    let mut identity = Map::new();
    if let Some(value) = index {
        identity.insert("index".to_string(), json!(value));
    }
    identity.insert("name".to_string(), json!(name));
    identity.insert(
        "gitSha".to_string(),
        if head.status.success() {
            json!(String::from_utf8_lossy(&head.stdout).trim())
        } else {
            JsonValue::Null
        },
    );
    identity.insert(
        "dirty".to_string(),
        json!(!diff.status.success() || !others.is_empty()),
    );
    identity.insert("worktreeSha256".to_string(), json!(worktree_sha));

    let mut exact = Map::new();
    if let Some(value) = index {
        exact.insert("index".to_string(), json!(value));
    }
    exact.insert("name".to_string(), json!(name));
    exact.insert(
        "worktreeSha256".to_string(),
        identity["worktreeSha256"].clone(),
    );
    let exact_bytes = serde_json::to_vec(&JsonValue::Object(exact))?;
    identity.insert(
        "sha256".to_string(),
        json!(crate::evidence::sha256_bytes(&exact_bytes)),
    );
    Ok(JsonValue::Object(identity))
}

pub(crate) fn submitted_source_identity(app_id: &str) -> Result<Option<JsonValue>, Failure> {
    let Some(file) = std::env::var_os("PROBIERZ_SOURCE_IDENTITY") else {
        return Ok(None);
    };
    let file = PathBuf::from(file);
    let parsed = crate::json_file::required(&file)
        .map_err(|error| Failure::config("run.source", error.detail))?;
    if parsed.get("schemaVersion").and_then(JsonValue::as_u64) != Some(IDENTITY_SCHEMA_VERSION) {
        return Err(Failure::config(
            "run.source",
            format!(
                "{}: schemaVersion must be {IDENTITY_SCHEMA_VERSION}, the source identity schema this Probierz writes and reads; it is {}",
                file.display(),
                parsed.get("schemaVersion").unwrap_or(&JsonValue::Null)
            ),
        ));
    }
    if !parsed
        .pointer("/harness/worktreeSha256")
        .and_then(JsonValue::as_str)
        .is_some_and(crate::evidence::is_sha256)
    {
        return Err(Failure::config(
            "run.source",
            format!("{}: harness worktreeSha256 is missing", file.display()),
        ));
    }
    if let Some(repositories) = parsed
        .pointer("/app/repositories")
        .and_then(JsonValue::as_array)
    {
        for repository in repositories {
            if !repository
                .get("worktreeSha256")
                .and_then(JsonValue::as_str)
                .is_some_and(crate::evidence::is_sha256)
            {
                let name = match repository.get("name").and_then(JsonValue::as_str) {
                    Some(name) => name,
                    None => "(the entry declares no name)",
                };
                return Err(Failure::config(
                    "run.source",
                    format!(
                        "{}: repository {name} has no worktreeSha256",
                        file.display()
                    ),
                ));
            }
        }
    }
    if parsed
        .get("appId")
        .and_then(JsonValue::as_str)
        .is_some_and(|declared| declared != app_id)
    {
        return Ok(None);
    }
    Ok(Some(parsed))
}

/// Exact path-independent harness and application identity.
pub fn app_source_identity(
    harness: &Path,
    app_id: &str,
    primary_root: Option<&Path>,
) -> Result<JsonValue, Failure> {
    if let Some(submitted) = submitted_source_identity(app_id)? {
        return Ok(submitted);
    }
    let loaded = manifest::load(harness, app_id)?;
    let repositories = loaded
        .document
        .get("repositories")
        .and_then(YamlValue::as_sequence)
        .ok_or_else(|| Failure::config("run.source", "manifest repositories are required"))?;
    // Every declared repository is hashed at its root; an entry naming none
    // is refused, never hashed as the working directory.
    let mut roots = repositories
        .iter()
        .enumerate()
        .map(|(index, repository)| {
            repository
                .get("root")
                .and_then(YamlValue::as_str)
                .map(Path::new)
                .ok_or_else(|| {
                    Failure::config(
                        "run.source",
                        format!("app {app_id}'s manifest repository {index} declares no root"),
                    )
                })
        })
        .collect::<Result<Vec<&Path>, Failure>>()?;
    // A given primary root stands in for the first declared repository.
    if let (Some(primary), Some(first)) = (primary_root, roots.first_mut()) {
        *first = primary;
    }
    let mut repository_values = Vec::with_capacity(roots.len());
    for (index, root) in roots.into_iter().enumerate() {
        let name = root.file_name().and_then(OsStr::to_str).ok_or_else(|| {
            Failure::config(
                "run.source",
                format!("repository root {} has no directory name", root.display()),
            )
        })?;
        repository_values.push(repository_identity(root, name, Some(index), false, false)?);
    }
    let concise: Vec<JsonValue> = repository_values
        .iter()
        .map(|repository| {
            json!({
                "index": repository["index"],
                "sha256": repository["sha256"],
            })
        })
        .collect();
    let app_sha = crate::evidence::sha256_bytes(&serde_json::to_vec(&concise)?);
    Ok(json!({
        "schemaVersion": IDENTITY_SCHEMA_VERSION,
        "appId": app_id,
        "harness": repository_identity(harness, "probierz", None, true, true)?,
        "app": { "sha256": app_sha, "repositories": repository_values },
    }))
}

pub fn source_identity_command(harness: &Path, app_id: &str) -> Result<(), Failure> {
    print_json(&app_source_identity(harness, app_id, None)?)
}
