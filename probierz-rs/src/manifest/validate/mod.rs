//! What a manifest has to satisfy, one section at a time: the document
//! itself, the repositories and surfaces it maps, the journeys it declares,
//! and the policies around them.

mod head;
mod journeys;
mod policies;
mod surfaces;

pub use head::*;
pub(crate) use journeys::*;
pub(crate) use policies::*;
pub(crate) use surfaces::*;
