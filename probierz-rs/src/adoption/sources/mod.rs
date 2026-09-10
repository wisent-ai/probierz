//! Where an adoption reads from and what it has already recorded: the
//! definitions a source tree offers, and the index of what was adopted.

mod definitions;
mod index;

pub(crate) use definitions::*;
pub(crate) use index::*;
