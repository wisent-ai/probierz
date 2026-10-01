//! The journeys this toolkit runs itself, and the runner that reports them.
//!
//! Three surfaces execute here rather than through an external test runner:
//! terminal applications, driven over a real PTY; native desktop
//! applications, driven through the accessibility tree; and web pages,
//! driven through a Weles browser (`weles mcp`). Each journey is a function
//! the runner calls, so a journey failure is a returned reason instead of a
//! child process exit status.
//!
//! The report this writes is the canonical one, so `analyze` treats every
//! surface alike.
//! | part | what it owns |
//! |---|---|
//! | [`context`] | one journey's context, the media it declares, and the registry it is selected from |
//! | [`external`] | a spec that is a file on disk rather than a function here |
//! | [`execute`] | running one journey and catching what it did |
//! | [`report`] | the canonical report written afterwards |
//!
//! Every part opens with `use crate::specs::*;`, so the list below is the
//! module's single import list.

pub(crate) use std::collections::BTreeMap;
pub(crate) use std::fs;
pub(crate) use std::io::Write;
pub(crate) use std::os::unix::fs::PermissionsExt;
pub(crate) use std::panic::{catch_unwind, AssertUnwindSafe};
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::Command;
pub(crate) use std::sync::Mutex;
pub(crate) use std::time::{Duration, Instant, SystemTime};

pub(crate) use serde::Serialize;
pub(crate) use serde_json::Value;

pub(crate) use crate::failure::{create_private, fail, iso_timestamp, Failure};

pub mod cua;
pub mod tui;
pub mod web;

mod runner;

pub use runner::*;

/// The one checkout of a Wisent repository on this machine. Every product
/// repository lives under the same directory in the operator's home, so a
/// journey names the repository and never the account it is checked out by.
pub(crate) fn wisent_checkout(name: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Documents/CodingProjects/Wisent").join(name)
}
