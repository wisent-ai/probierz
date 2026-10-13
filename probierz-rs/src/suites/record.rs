//! Where a suite run is kept and how it is read back.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::failure::Failure;

const RUN_FILE: &str = "run.json";

/// A fresh directory for one run, named by its start and Git's own short
/// form of its revision so runs sort by time and read their source at a
/// glance; a name already taken is refused rather than reused.
pub(crate) fn create_run_directory(
    root: &Path,
    started_at: &str,
    short_revision: &str,
) -> Result<PathBuf, Failure> {
    let stamp: String = started_at
        .chars()
        .map(|character| if character == ':' { '-' } else { character })
        .collect();
    let directory = root.join(format!("{stamp}-{short_revision}"));
    std::fs::create_dir_all(root)?;
    std::fs::create_dir(&directory).map_err(|error| {
        Failure::config(
            "suites.record",
            format!("{} could not be created: {error}", directory.display()),
        )
    })?;
    Ok(directory)
}

pub(crate) fn write_run(directory: &Path, document: &Value) -> Result<(), Failure> {
    let path = directory.join(RUN_FILE);
    let body = serde_json::to_vec_pretty(document)?;
    std::fs::write(&path, body).map_err(|error| {
        Failure::config(
            "suites.record",
            format!("{} could not be written: {error}", path.display()),
        )
    })
}

/// The newest run under `root`, by directory name; none is refused naming
/// the directory looked in.
pub(crate) fn newest(root: &Path) -> Result<Value, Failure> {
    let entries = std::fs::read_dir(root).map_err(|error| {
        Failure::config(
            "suites.record",
            format!("no suite run is recorded under {}: {error}", root.display()),
        )
    })?;
    let mut names: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            names.push(entry.path());
        }
    }
    names.sort();
    let Some(latest) = names.last() else {
        return Err(Failure::config(
            "suites.record",
            format!("no suite run is recorded under {}", root.display()),
        ));
    };
    let path = latest.join(RUN_FILE);
    let text = std::fs::read_to_string(&path).map_err(|error| {
        Failure::config(
            "suites.record",
            format!("{} could not be read: {error}", path.display()),
        )
    })?;
    serde_json::from_str(&text).map_err(|error| {
        Failure::config(
            "suites.record",
            format!("{} is not a run record: {error}", path.display()),
        )
    })
}

/// Copy a file or a whole directory, every entry as it is.
pub(crate) fn copy_tree(source: &Path, target: &Path) -> Result<(), Failure> {
    if source.is_dir() {
        std::fs::create_dir_all(target)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
        return Ok(());
    }
    std::fs::copy(source, target).map_err(|error| {
        Failure::config(
            "suites.record",
            format!(
                "{} could not be copied to {}: {error}",
                source.display(),
                target.display()
            ),
        )
    })?;
    Ok(())
}
