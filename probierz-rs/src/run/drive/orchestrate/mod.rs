//! Fanning out runs: the CI pass over the affected targets, the declared
//! matrix a profile expands into, and the dispatch that runs each cell.

mod ci;
mod dispatch;
mod plan;

pub(crate) use ci::*;
pub(crate) use dispatch::*;
pub(crate) use plan::*;
