//! The signed receipt: what it records about a run, the source and build
//! identity it names, the signature over all of it, and the verification a
//! reader performs.

mod identity;
mod records;
mod sign;
mod verify;

pub(crate) use identity::*;
pub(crate) use records::*;
pub use sign::*;
pub use verify::*;

/// The schema of a signed evidence receipt: the one `receipt` signs, and the
/// first that carries publication provenance, which a publication requires.
pub(crate) const RECEIPT_SCHEMA_VERSION: u64 = 3;
