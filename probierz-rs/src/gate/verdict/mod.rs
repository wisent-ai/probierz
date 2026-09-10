//! The verdict: the evidence it is made of, the decision made from that
//! evidence, and the commands that print, enforce or activate it.

mod commands;
mod decide;
mod evidence;

pub use commands::*;
pub(crate) use decide::*;
pub(crate) use evidence::*;
