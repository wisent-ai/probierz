//! What a finished run leaves behind: the completed manifest, and the
//! registered entry point that hands a caller the result.

mod manifest;
mod registered;

pub(crate) use manifest::*;
pub(crate) use registered::*;
