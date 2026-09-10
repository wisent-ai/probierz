//! The machine around the suite: the declared data commands run beside it,
//! the reports written from what it answered, and the diagnostics collected
//! while it ran.

mod data_command;
mod diagnostics;
mod reports;

pub(crate) use data_command::*;
pub(crate) use diagnostics::*;
pub(crate) use reports::*;
