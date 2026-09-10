//! The pre-push gate: what the repository says has changed, the verdict that
//! makes, and the hook that runs it.

mod hook;
mod repository;
mod verdict;

pub use hook::*;
pub(crate) use repository::*;
pub(crate) use verdict::*;
