use serde_json::json;
use crate::adoption::*;
pub fn adopt_project(
    project_root: &Path,
    source_root: &Path,
    replace: bool,
) -> Result<Value, Failure> {
    let destination = repository_root(project_root, "Probierz project root")?;
    let source = repository_root(source_root, "Adoption source")?;
    if source == destination {
        return Err(fail(
            "adoption.source",
            "adoption source is already this Probierz project",
        ));
    }

    let definitions = source_definitions(&source)?;
    let mut index = read_index(&destination)?;
    let source_root_text = path_text(&source);
    let source_key = sha256(source_root_text.as_bytes());
    let existing_source = index
        .sources
        .iter()
        .find(|entry| entry.source_key == source_key);
    let previous_by_path: HashMap<&str, &RetainedFile> = existing_source
        .map(|entry| {
            entry
                .files
                .iter()
                .map(|file| (file.path.as_str(), file))
                .collect()
        })
        .unwrap_or_default();
    let ownership = file_owners(&index);
    let incoming: BTreeSet<&str> = definitions
        .files
        .iter()
        .map(|file| file.relative.as_str())
        .collect();
    let mut conflicts = Vec::new();
    let mut planned = Vec::new();
    let mut unchanged = 0usize;

    for file in &definitions.files {
        let target = absolute(&destination, &file.relative)?;
        let current = current_file(&target)?;
        let other_owner = ownership
            .get(file.relative.as_str())
            .is_some_and(|owners| owners.iter().any(|owner| owner != &source_key));
        if other_owner {
            conflicts.push(conflict(
                &file.relative,
                "destination is owned by another adopted source",
                current.digest_value(),
                Value::String(file.sha256.clone()),
            ));
            continue;
        }
        if let CurrentFile::Regular { sha256, mode } = &current {
            if sha256 == &file.sha256 && *mode == file.mode {
                unchanged += 1;
                continue;
            }
        }
        let previous = previous_by_path.get(file.relative.as_str()).copied();
        let locally_changed = match (&current, previous) {
            (CurrentFile::Regular { sha256, mode }, Some(previous)) => {
                sha256 != &previous.sha256 || *mode != previous.mode
            }
            _ => false,
        };
        if matches!(current, CurrentFile::Unsupported) {
            conflicts.push(conflict(
                &file.relative,
                "destination is not a regular file",
                Value::String("unsupported".to_string()),
                Value::String(file.sha256.clone()),
            ));
        } else if locally_changed {
            conflicts.push(conflict(
                &file.relative,
                "previously adopted definition has local content or mode changes",
                current.digest_value(),
                Value::String(file.sha256.clone()),
            ));
        } else if !matches!(current, CurrentFile::Missing) && !replace {
            conflicts.push(conflict(
                &file.relative,
                "destination content or mode differs; repeat with explicit replacement",
                current.digest_value(),
                Value::String(file.sha256.clone()),
            ));
        } else {
            planned.push(file);
        }
    }

    let mut removals = Vec::new();
    if let Some(existing) = existing_source {
        for previous in &existing.files {
            if incoming.contains(previous.path.as_str()) {
                continue;
            }
            let target = absolute(&destination, &previous.path)?;
            let current = current_file(&target)?;
            if matches!(current, CurrentFile::Missing) {
                continue;
            }
            if !replace {
                conflicts.push(conflict(
                    &previous.path,
                    "previously adopted definition is absent from the selected source",
                    current.digest_value(),
                    Value::Null,
                ));
            } else if !matches!(
                &current,
                CurrentFile::Regular { sha256, mode }
                    if sha256 == &previous.sha256 && *mode == previous.mode
            ) {
                conflicts.push(conflict(
                    &previous.path,
                    "previously adopted definition has local content or mode changes",
                    current.digest_value(),
                    Value::Null,
                ));
            } else {
                removals.push(previous.path.clone());
            }
        }
    }

    if !conflicts.is_empty() {
        let rejected = conflicts.len();
        return Ok(result(
            "conflict",
            &source_root_text,
            &definitions,
            Counts {
                imported: 0,
                unchanged,
                removed: 0,
                rejected,
            },
            &conflicts,
        ));
    }

    if existing_source.is_some_and(|entry| entry.source_digest == definitions.source_digest)
        && planned.is_empty()
        && removals.is_empty()
    {
        return Ok(result(
            "unchanged",
            &source_root_text,
            &definitions,
            Counts {
                imported: 0,
                unchanged,
                removed: 0,
                rejected: 0,
            },
            &[],
        ));
    }

    let had_existing_source = existing_source.is_some();
    let record = SourceRecord {
        source_key: source_key.clone(),
        source_root: source_root_text.clone(),
        source_digest: definitions.source_digest.clone(),
        adopted_at: now_iso(),
        applications: definitions.application_ids.clone(),
        files: definitions
            .files
            .iter()
            .map(|file| RetainedFile {
                path: file.relative.clone(),
                sha256: file.sha256.clone(),
                mode: file.mode,
            })
            .collect(),
    };
    index.sources.retain(|entry| entry.source_key != source_key);
    index.sources.push(record);
    index
        .sources
        .sort_by(|left, right| left.source_root.cmp(&right.source_root));

    apply_transaction(&destination, &planned, &removals, &index)?;
    Ok(result(
        if had_existing_source {
            "replaced"
        } else {
            "imported"
        },
        &source_root_text,
        &definitions,
        Counts {
            imported: planned.len(),
            unchanged,
            removed: removals.len(),
            rejected: 0,
        },
        &[],
    ))
}

/// Read retained source identities without exposing adopted definition contents.
pub fn list_project_adoptions(project_root: &Path) -> Result<Value, Failure> {
    let root = repository_root(project_root, "Probierz project root")?;
    let index = read_index(&root)?;
    let sources: Vec<Value> = index
        .sources
        .into_iter()
        .map(|source| {
            json!({
                "sourceKey": source.source_key,
                "sourceRoot": source.source_root,
                "sourceDigest": source.source_digest,
                "adoptedAt": source.adopted_at,
                "applications": source.applications,
                "fileCount": source.files.len(),
            })
        })
        .collect();
    Ok(json!({
        "schema": INDEX_SCHEMA,
        "file": path_text(&root.join(INDEX_RELATIVE_PATH)),
        "sources": sources,
    }))
}

