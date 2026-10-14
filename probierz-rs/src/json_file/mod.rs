//! Reading a JSON document a run or a step left on disk. A file that is not
//! there is `None`, the one absence a reader may act on; a file that cannot
//! be read, or does not hold JSON, is a failure naming the file and its
//! cause, never read as absent.

use std::path::Path;

use serde_json::Value;

use crate::failure::Failure;

/// The JSON document at `path`, `None` when no file is there.
pub(crate) fn read(path: &Path) -> Result<Option<Value>, Failure> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Failure::config(
                "json.read",
                format!("{} cannot be read: {error}", path.display()),
            ))
        }
    };
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        Failure::invalid(
            "json.read",
            format!("{} does not hold JSON: {error}", path.display()),
        )
    })
}
