//! Sending one run to a fleet host: the script the host executes, the
//! submission itself, and the watch that follows the job to a terminal
//! state.

mod machine;
mod run;
mod script;
mod watch;

pub(crate) use machine::*;
pub(crate) use run::*;
pub(crate) use script::*;
pub(crate) use watch::*;
