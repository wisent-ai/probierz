//! Whether a path is there, a path that cannot be examined being an error.

use std::io;
use std::path::Path;

/// Whether something is at `path`, a dangling link included; a path that
/// cannot be examined is an error of the same kind naming the path.
/// `Path::exists` answers false for both, so a file that could not be
/// examined was overwritten, or moved aside, as if there were none.
pub(crate) fn present(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_metadata) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!("{} cannot be examined: {error}", path.display()),
        )),
    }
}
