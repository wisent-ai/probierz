//! The signed receipt: what it records about a run, the source and build
//! identity it names, the signature over all of it, and the verification a
//! reader performs.

mod identity;
mod records;
mod sign;
mod verify;

pub use sign::*;
pub use verify::*;
pub(crate) use identity::*;
pub(crate) use records::*;
