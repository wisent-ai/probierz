//! The remote half of authoring: the two submissions that start a job, the
//! restore that brings its result back, and the install that puts an
//! authored spec where the product expects it.

mod author;
mod install;
mod restore;
mod seo;

pub(crate) use author::*;
pub(crate) use install::*;
pub(crate) use restore::*;
pub(crate) use seo::*;
