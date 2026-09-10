//! What a run is bound to: the names and ids it is filed under, the source
//! and build it came from, and the records and hashes written beside it.

mod naming;
mod records;
mod source;

pub(crate) use naming::*;
pub(crate) use records::*;
pub(crate) use source::*;
