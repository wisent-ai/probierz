//! Read-only discovery: what exists, not what happened.
//!
//! Nothing here starts a driver, installs a dependency, executes a suite or
//! touches an application repository. Running a journey needs Chromium, Appium
//! or a simulator, and keeping that out of the read surface is why an operator
//! can ask these questions on any machine.
//!
//! | part | what it owns |
//! |---|---|
//! | [`surfaces`] | the surfaces this toolkit drives, and the applications declared against them |
//! | [`specs`] | the spec files on disk and the outline of one of them |
//! | [`commands`] | the exact command a target runs, and the hosts it can run on |

pub(crate) use std::path::{Path, PathBuf};

pub(crate) use serde::Serialize;
pub(crate) use serde_yaml::Value;

pub(crate) use crate::failure::{print_json, Answer, Failure};
pub(crate) use crate::manifest;

mod commands;
mod specs;
mod surfaces;

pub use commands::*;
pub use specs::*;
pub use surfaces::*;
