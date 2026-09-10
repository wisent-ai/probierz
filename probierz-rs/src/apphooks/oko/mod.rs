//! The Oko fixture these hooks build and take down: the technical account it
//! runs as, the documents it seeds, the feedback applied to them, and the
//! verification and cleanup afterwards.

mod account;
mod feedback;
mod fixture;
mod lifecycle;

pub(crate) use account::*;
pub(crate) use feedback::*;
pub(crate) use fixture::*;
pub(crate) use lifecycle::*;
