use crate::stado::*;
pub fn source_file_list(root: &Path) -> Result<Vec<u8>, Failure> {
    let output = sh(
        "git",
        &[
            "-C".into(),
            root.display().to_string(),
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
    if output.status != Some(0) {
        return Err(Failure::config(
            "run.source",
            format!(
                "git ls-files in {}: {}",
                root.display(),
                process_text(&output)
            ),
        ));
    }
    let mut files: Vec<String> = output
        .stdout
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .filter(|relative| {
            let path = Path::new(relative);
            let secret = path
                .components()
                .any(|part| part.as_os_str().to_string_lossy().starts_with(".env"))
                || path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("probierz-") && name.ends_with(".json"));
            let excluded = path.components().any(|part| {
                matches!(
                    part.as_os_str().to_str(),
                    Some("node_modules" | "test-results" | "..")
                )
            });
            !secret
                && !excluded
                && fs::symlink_metadata(root.join(path))
                    .map(|metadata| metadata.is_file() || metadata.file_type().is_symlink())
                    .unwrap_or(false)
        })
        .map(str::to_string)
        .collect();
    if root.join("package-lock.json").exists()
        && !files.iter().any(|path| path == "package-lock.json")
    {
        files.push("package-lock.json".into());
    }
    files.sort();
    let mut answer = Vec::new();
    for file in files {
        answer.extend(file.as_bytes());
        answer.push(0);
    }
    Ok(answer)
}

