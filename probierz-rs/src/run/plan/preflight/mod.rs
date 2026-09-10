//! Whether a machine can run a target at all: the probes that ask it, the
//! gate that decides, and the setup that installs what is missing.

mod gate;
mod probes;
mod setup;

pub(crate) use gate::*;
pub(crate) use probes::*;
pub(crate) use setup::*;
