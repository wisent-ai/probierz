//! The suite while it runs: the child process and its output, the media it
//! leaves behind, and what the machine itself reported alongside it.

mod capture;
mod media;
mod platform;

pub(crate) use capture::*;
pub(crate) use media::*;
pub(crate) use platform::*;
