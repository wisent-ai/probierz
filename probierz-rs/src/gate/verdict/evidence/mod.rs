//! What a verdict is made of: the runs it read, the declared matrix they
//! must cover, and the signed receipt a release gate stands on.

mod inspect;
mod matrix;
mod receipt;
mod runs;

pub(crate) use inspect::*;
pub(crate) use matrix::*;
pub(crate) use receipt::*;
pub(crate) use runs::*;
