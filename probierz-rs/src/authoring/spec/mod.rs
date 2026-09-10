//! Authoring one spec: the draft a model produces, the accepted spec
//! installed from it, the journey it is run as, and the manifest around it.

mod author;
mod draft;
mod journey;
mod manifest;

pub use author::*;
pub use journey::*;
pub use manifest::*;
pub(crate) use draft::*;
