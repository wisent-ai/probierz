//! What the run is turned into afterwards: one report shape whatever driver
//! wrote it, the timeline its traces and logs make, the verdict and
//! diagnostics read off both, and the source and artifact identity the whole
//! record is bound to.

mod analysis;
mod events;
pub(crate) mod identity;
mod normalize;

pub(crate) use analysis::*;
pub(crate) use events::*;
pub(crate) use identity::*;
pub(crate) use normalize::*;
