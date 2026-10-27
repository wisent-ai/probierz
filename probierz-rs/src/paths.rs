//! The directory a path keeps its file in, and whether a path is there.

use std::io;
use std::path::Path;

/// The directory that holds `path`. A path that names no directory (the
/// filesystem root or an empty path) is refused by name, never taken to mean
/// the directory Probierz runs from. A bare file name is held by the empty
/// relative directory, which is where the caller named it.
pub(crate) fn directory_of(path: &Path) -> Result<&Path, String> {
    path.parent().ok_or_else(|| {
        format!(
            "the path {:?} names no directory to hold it",
            path.display().to_string()
        )
    })
}

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
