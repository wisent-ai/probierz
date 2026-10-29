//! Which round of a bounded authoring loop a brief is written for.

use std::fmt;

/// Round `number` of `of`: the attempt a brief asks for and how many the
/// loop allows.
#[derive(Clone, Copy)]
pub(crate) struct Round {
    pub(crate) number: u32,
    pub(crate) of: u32,
}

impl fmt::Display for Round {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Round {} of {}", self.number, self.of)
    }
}
