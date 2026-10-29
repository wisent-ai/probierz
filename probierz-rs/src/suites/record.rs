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

/// Copy one declared report without following a symbolic link in its
/// checkout-relative ancestry or in a directory it contains. A report that
/// was not produced is distinct from one that exists but cannot be copied.
pub(crate) fn copy_report(repo: &Path, declared: &str, target: &Path) -> Result<bool, Failure> {
    let mut source = repo.to_path_buf();
    for component in Path::new(declared).components() {
        source.push(component);
        if inspect(&source)?.is_none() {
            return Ok(false);
        }
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            Failure::config(
                "suites.record",
                format!("{} could not be created: {error}", parent.display()),
            )
        })?;
    }
    copy_tree(&source, target)?;
    Ok(true)
}

fn inspect(path: &Path) -> Result<Option<std::fs::Metadata>, Failure> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Failure::config(
                "suites.record",
                format!("{} could not be inspected: {error}", path.display()),
            ))
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(Failure::config(
            "suites.record",
            format!(
                "{} is a symbolic link, not a report inside the checkout",
                path.display()
            ),
        ));
    }
    Ok(Some(metadata))
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), Failure> {
    let metadata = inspect(source)?.ok_or_else(|| {
        Failure::config(
            "suites.record",
            format!(
                "{} disappeared while copying to {}",
                source.display(),
                target.display()
            ),
        )
    })?;
    if metadata.is_dir() {
        std::fs::create_dir_all(target).map_err(|error| {
            Failure::config(
                "suites.record",
                format!("{} could not be created: {error}", target.display()),
            )
        })?;
        let entries = std::fs::read_dir(source).map_err(|error| {
            Failure::config(
                "suites.record",
                format!("{} could not be listed: {error}", source.display()),
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                Failure::config(
                    "suites.record",
                    format!(
                        "{} could not yield its next entry: {error}",
                        source.display()
                    ),
                )
            })?;
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
