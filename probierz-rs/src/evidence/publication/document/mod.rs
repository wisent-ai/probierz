//! The publication document itself: the checks and assembly, the assets
//! registered in it, and the identifiers those assets must use.

mod assets;
mod create;
mod validators;

pub(crate) use assets::*;
pub(crate) use create::*;
pub(crate) use validators::*;
