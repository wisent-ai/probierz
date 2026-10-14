//! The run record's merge: the one `update_json` every caller of
//! `crate::run::*` writes a run manifest through.

use crate::run::*;
use serde_json::json;

/// Merge `patch` into the JSON object at `file`. Only a missing file starts
/// empty: one that cannot be read or parsed is refused naming it, because
/// merged into an empty object and written back it replaced the run's whole
/// record (identity, spec, conditions, seed) with the patch alone.
pub(crate) fn update_json(file: &Path, patch: &Value) -> Result<(), Failure> {
    let mut current = match crate::json_file::read(file)? {
        Some(current) => current,
        None => json!({}),
    };
    let target = current.as_object_mut().ok_or_else(|| {
        Failure::config(
            "run.record.update",
            format!("{} must contain a JSON object", file.display()),
        )
    })?;
    let values = patch.as_object().ok_or_else(|| {
        Failure::config(
            "run.record.update",
            format!("{} update patch must be a JSON object", file.display()),
        )
    })?;
    target.extend(values.clone());
    write_json(file, &current)
}

#[cfg(test)]
#[path = "../../../../../tests/records/update.rs"]
mod tests;
