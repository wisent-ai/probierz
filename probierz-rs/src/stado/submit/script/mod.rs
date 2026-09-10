//! The one shell program a fleet host runs for a submission: its preamble
//! and the body that depends on the target, the mode and the provisioning.

mod body;
mod build;

pub(crate) use body::*;
pub(crate) use build::*;
