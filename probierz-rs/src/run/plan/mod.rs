//! What a run is before anything starts: which target it names, what it was
//! asked for, what the machine must already have, and which targets a set of
//! changed files selects.

mod affected;
mod preflight;
mod targets;

pub(crate) use affected::*;
pub(crate) use preflight::*;
pub(crate) use targets::*;
