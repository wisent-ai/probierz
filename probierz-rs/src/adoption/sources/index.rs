use crate::adoption::*;
pub(crate) fn files_below(root: &Path, relative_root: &str) -> Result<Vec<DefinitionFile>, Failure> {
    let start = absolute(root, relative_root)?;
    let metadata = fs::symlink_metadata(&start)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(fail(
            "adoption.source",
            format!(
                "unsupported project definition directory: {}",
                start.display()
            ),
        ));
    }
    let mut pending = vec![relative_root.to_string()];
    let mut files = Vec::new();
    while let Some(current) = pending.pop() {
        let directory = absolute(root, &current)?;
        let mut entries: Vec<_> = fs::read_dir(directory)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = os_text(&entry.file_name()).to_string();
            let relative = format!("{current}/{name}");
            let source = absolute(root, &relative)?;
            let metadata = fs::symlink_metadata(&source)?;
            if metadata.file_type().is_symlink() {
                return Err(fail(
                    "adoption.source",
                    format!(
                        "project definitions must not contain symlinks: {}",
                        source.display()
                    ),
                ));
            }
            if metadata.is_dir() {
                pending.push(relative);
            } else if metadata.is_file() {
                files.push(DefinitionFile {
                    relative,
                    source,
                    mode: metadata_mode(&metadata),
                    bytes: Vec::new(),
                    sha256: String::new(),
                });
            } else {
                return Err(fail(
                    "adoption.source",
                    format!(
                        "project definitions contain an unsupported filesystem entry: {}",
                        source.display()
                    ),
                ));
            }
        }
    }
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}

pub(crate) fn read_index(project_root: &Path) -> Result<AdoptionIndex, Failure> {
    let file = absolute(project_root, INDEX_RELATIVE_PATH)?;
    if !file.exists() {
        return Ok(AdoptionIndex {
            schema: INDEX_SCHEMA.to_string(),
            sources: Vec::new(),
        });
    }
    let metadata = fs::symlink_metadata(&file)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(fail(
            "adoption.index",
            format!(
                "Probierz adoption index is not a regular file: {}",
                file.display()
            ),
        ));
    }
    let document: Value = serde_json::from_slice(&fs::read(&file)?)?;
    if document.get("schema").and_then(Value::as_str) != Some(INDEX_SCHEMA)
        || !document.get("sources").is_some_and(Value::is_array)
    {
        return Err(fail(
            "adoption.index",
            format!("unsupported Probierz adoption index: {}", file.display()),
        ));
    }
    validate_index_records(&document, project_root, &file)?;
    serde_json::from_value(document).map_err(|_| {
        fail(
            "adoption.index",
            format!(
                "invalid Probierz adoption source record: {}",
                file.display()
            ),
        )
    })
}

pub(crate) fn validate_index_records(
    document: &Value,
    project_root: &Path,
    file: &Path,
) -> Result<(), Failure> {
    for source in document["sources"].as_array().expect("checked") {
        let valid_source = source
            .get("sourceKey")
            .and_then(Value::as_str)
            .is_some_and(is_sha256)
            && source.get("sourceRoot").is_some_and(Value::is_string)
            && source.get("files").is_some_and(Value::is_array);
        if !valid_source {
            return Err(fail(
                "adoption.index",
                format!(
                    "invalid Probierz adoption source record: {}",
                    file.display()
                ),
            ));
        }
        for retained in source["files"].as_array().expect("checked") {
            let valid_file = retained.get("path").is_some_and(Value::is_string)
                && retained
                    .get("sha256")
                    .and_then(Value::as_str)
                    .is_some_and(is_sha256)
                && retained.get("mode").and_then(Value::as_u64).is_some();
            if !valid_file {
                return Err(fail(
                    "adoption.index",
                    format!("invalid Probierz adoption file record: {}", file.display()),
                ));
            }
            absolute(
                project_root,
                retained["path"].as_str().expect("validated string"),
            )?;
        }
    }
    Ok(())
}

pub(crate) fn file_owners(index: &AdoptionIndex) -> HashMap<&str, Vec<&str>> {
    let mut owners: HashMap<&str, Vec<&str>> = HashMap::new();
    for source in &index.sources {
        for file in &source.files {
            owners
                .entry(file.path.as_str())
                .or_default()
                .push(source.source_key.as_str());
        }
    }
    owners
}

pub(crate) enum CurrentFile {
    Missing,
    Unsupported,
    Regular { sha256: String, mode: u32 },
}

impl CurrentFile {
    pub(crate) fn digest_value(&self) -> Value {
        match self {
            Self::Missing => Value::Null,
            Self::Unsupported => Value::String("unsupported".to_string()),
            Self::Regular { sha256, .. } => Value::String(sha256.clone()),
        }
    }
}

pub(crate) fn current_file(file: &Path) -> Result<CurrentFile, Failure> {
    let metadata = match fs::symlink_metadata(file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CurrentFile::Missing)
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(CurrentFile::Unsupported);
    }
    Ok(CurrentFile::Regular {
        sha256: sha256_file(file)?,
        mode: metadata_mode(&metadata),
    })
}

