use serde_json::json;
use crate::adoption::*;
pub(crate) fn conflict(
    path: &str,
    reason: &'static str,
    existing_sha256: Value,
    incoming_sha256: Value,
) -> Conflict {
    Conflict {
        path: path.to_string(),
        reason,
        existing_sha256,
        incoming_sha256,
    }
}

pub(crate) fn result(
    status: &str,
    source_root: &str,
    definitions: &Definitions,
    counts: Counts,
    conflicts: &[Conflict],
) -> Value {
    json!({
        "schema": RESULT_SCHEMA,
        "status": status,
        "sourceRoot": source_root,
        "sourceDigest": definitions.source_digest,
        "applications": definitions.application_ids,
        "imported": counts.imported,
        "unchanged": counts.unchanged,
        "removed": counts.removed,
        "conflicting": conflicts.len(),
        "rejected": counts.rejected,
        "conflicts": conflicts.iter().map(|item| json!({
            "path": item.path,
            "reason": item.reason,
            "existingSha256": item.existing_sha256,
            "incomingSha256": item.incoming_sha256,
        })).collect::<Vec<_>>(),
        "skippedLocalState": definitions.skipped_local_state,
        "executedJourneys": false,
    })
}

pub(crate) fn apply_transaction(
    destination: &Path,
    planned: &[&DefinitionFile],
    removals: &[String],
    index: &AdoptionIndex,
) -> Result<(), Failure> {
    let transaction_id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let stage_root = destination.join(format!(".probierz-adoption-stage-{transaction_id}"));
    let backup_root = destination.join(format!(".probierz-adoption-backup-{transaction_id}"));
    let index_bytes = format!("{}\n", serde_json::to_string_pretty(index)?).into_bytes();
    let mut backed_up: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut placed: Vec<PathBuf> = Vec::new();

    let operation = (|| -> Result<(), Failure> {
        for file in planned {
            let staged_file = absolute(&stage_root, &file.relative)?;
            if let Some(parent) = staged_file.parent() {
                fs::create_dir_all(parent)?;
            }
            write_new(&staged_file, &file.bytes, file.mode)?;
            set_mode(&staged_file, file.mode)?;
        }
        let staged_index = absolute(&stage_root, INDEX_RELATIVE_PATH)?;
        if let Some(parent) = staged_index.parent() {
            fs::create_dir_all(parent)?;
        }
        write_private(&staged_index, &index_bytes)?;
        set_mode(&staged_index, 0o600)?;

        for relative in removals {
            let target = absolute(destination, relative)?;
            let backup = absolute(&backup_root, relative)?;
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&target, &backup)?;
            backed_up.push((target, backup));
        }
        for relative in planned
            .iter()
            .map(|file| file.relative.as_str())
            .chain(std::iter::once(INDEX_RELATIVE_PATH))
        {
            let target = absolute(destination, relative)?;
            let staged_file = absolute(&stage_root, relative)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            if target.exists() {
                let backup = absolute(&backup_root, relative)?;
                if let Some(parent) = backup.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(&target, &backup)?;
                backed_up.push((target.clone(), backup));
            }
            fs::rename(staged_file, &target)?;
            placed.push(target);
        }
        Ok(())
    })();

    if operation.is_err() {
        for target in placed.iter().rev() {
            let _ = fs::remove_file(target);
        }
        for (target, backup) in backed_up.iter().rev() {
            if backup.exists() {
                if let Some(parent) = target.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::rename(backup, target);
            }
        }
    }
    let _ = fs::remove_dir_all(&stage_root);
    let _ = fs::remove_dir_all(&backup_root);
    operation
}

pub(crate) fn repository_root(value: &Path, label: &str) -> Result<PathBuf, Failure> {
    let canonical = fs::canonicalize(value).map_err(|_| {
        fail(
            "adoption.repository",
            format!("{label} is not an existing directory: {}", value.display()),
        )
    })?;
    let metadata = fs::symlink_metadata(&canonical)?;
    if !metadata.is_dir() {
        return Err(fail(
            "adoption.repository",
            format!("{label} is not a directory: {}", canonical.display()),
        ));
    }
    let git = canonical.join(".git");
    let git_metadata = match fs::symlink_metadata(&git) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Err(fail(
                "adoption.repository",
                format!("{label} is not a Git repository: {}", canonical.display()),
            ))
        }
    };
    if git_metadata.file_type().is_symlink() || (!git_metadata.is_dir() && !git_metadata.is_file())
    {
        return Err(fail(
            "adoption.repository",
            format!("{label} has an unsupported .git entry: {}", git.display()),
        ));
    }
    Ok(canonical)
}

pub(crate) fn absolute(root: &Path, relative: &str) -> Result<PathBuf, Failure> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(fail(
            "adoption.path",
            format!("project definition path escapes its repository: {relative}"),
        ));
    }
    Ok(root.join(path))
}

