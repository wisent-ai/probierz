use serde_json::json;
use crate::authoring::*;
pub(crate) const PROBE_CHARS: usize = 9_000;
pub(crate) const BODY_CHARS: usize = 1_500;
pub(crate) const MAX_PATCH_CHARS: usize = 80_000;
pub(crate) const MAX_CHANGED_FILES: usize = 8;

pub(crate) fn command_output(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
) -> std::io::Result<std::process::Output> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(directory) = cwd {
        command.current_dir(directory);
    }
    command.output()
}

pub(crate) fn git_paths(root: &Path, others_only: bool) -> Result<Vec<String>, Failure> {
    let mut command = Command::new("git");
    command.arg("-C").arg(root).arg("ls-files");
    if !others_only {
        command.arg("--cached");
    }
    command.args(["--others", "--exclude-standard", "-z"]);
    let listed = command
        .output()
        .map_err(|error| source_git_failure(root, error.to_string()))?;
    if !listed.status.success() {
        let reason = String::from_utf8_lossy(&listed.stderr).trim().to_string();
        return Err(source_git_failure(
            root,
            if reason.is_empty() {
                format!(
                    "git exited {}",
                    listed
                        .status
                        .code()
                        .map_or_else(|| "null".to_string(), |value| value.to_string())
                )
            } else {
                reason
            },
        ));
    }
    Ok(listed
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect())
}

pub(crate) fn source_git_failure(root: &Path, reason: String) -> Failure {
    let detail = if root.exists() {
        format!("git ls-files in {}: {reason}", root.display())
    } else {
        format!("git ls-files in {}: {reason}", root.display())
    };
    Failure::config("run.source", detail)
}

pub(crate) fn source_path_allowed(relative: &str, exclude_runtime_secrets: bool) -> bool {
    if relative.is_empty() || Path::new(relative).is_absolute() {
        return false;
    }
    let parts: Vec<&str> = relative.split('/').collect();
    if parts
        .iter()
        .any(|part| matches!(*part, ".." | "node_modules" | "test-results"))
    {
        return false;
    }
    if !exclude_runtime_secrets {
        return true;
    }
    if parts.iter().any(|part| part.starts_with(".env")) {
        return false;
    }
    let basename = parts.last().copied().unwrap_or_default();
    !(basename.starts_with("probierz-") && basename.ends_with(".json"))
}

pub(crate) fn repository_source_files(
    root: &Path,
    exclude_runtime_secrets: bool,
    include_package_lock: bool,
) -> Result<Vec<String>, Failure> {
    let mut files = BTreeSet::new();
    for relative in git_paths(root, false)? {
        if !source_path_allowed(&relative, exclude_runtime_secrets) {
            continue;
        }
        if let Ok(metadata) = fs::symlink_metadata(root.join(&relative)) {
            if metadata.is_file() || metadata.file_type().is_symlink() {
                files.insert(relative);
            }
        }
    }
    if include_package_lock && root.join("package-lock.json").exists() {
        files.insert("package-lock.json".to_string());
    }
    Ok(files.into_iter().collect())
}

pub(crate) fn hash_source_files(root: &Path, files: &[String]) -> Result<String, Failure> {
    let mut hash = Sha256::new();
    for relative in files {
        let file = root.join(relative);
        let metadata = fs::symlink_metadata(&file)?;
        let (kind, payload) = if metadata.file_type().is_symlink() {
            let target = fs::read_link(&file)?;
            ("symlink", target.as_os_str().as_encoded_bytes().to_vec())
        } else {
            ("file", fs::read(&file)?)
        };
        let header = serde_json::to_string(&json!({
            "path": relative,
            "kind": kind,
            "mode": metadata.mode() & 0o777,
            "bytes": payload.len(),
        }))?;
        hash.update(header.len().to_string().as_bytes());
        hash.update(b":");
        hash.update(header.as_bytes());
        hash.update(payload);
    }
    Ok(hex::encode(hash.finalize()))
}

