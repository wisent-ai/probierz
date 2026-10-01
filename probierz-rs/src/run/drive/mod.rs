//! Actually running things: one surface end to end, and the CI and
//! declared-matrix fan-out that call it repeatedly.

mod execute;
mod orchestrate;

pub(crate) use execute::*;
pub(crate) use orchestrate::*;
