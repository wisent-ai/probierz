//! Which host the login journey runs on: how a selector resolves to one,
//! and the quarantine that keeps a host that failed out of the next run.

mod quarantine;
mod target;

pub(crate) use quarantine::*;
pub(crate) use target::*;
