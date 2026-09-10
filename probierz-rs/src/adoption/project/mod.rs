//! One project's adoption: the records it is filed as, the adoption and
//! listing commands, the conflicts a file already on disk causes, and the
//! paths, modes and digests all of that is expressed in.

mod adopt;
mod conflicts;
mod paths;
mod records;

pub use adopt::*;
pub(crate) use conflicts::*;
pub(crate) use paths::*;
pub(crate) use records::*;
