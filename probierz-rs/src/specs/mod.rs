//! The journeys this toolkit runs itself, and the runner that reports them.
//!
//! Two surfaces execute here rather than through a browser driver: terminal
//! applications, driven over a real PTY, and native desktop applications,
//! driven through the accessibility tree. Both used to be Node processes that
//! a Node runner spawned one per journey. They are functions now, and the
//! runner calls them, so a journey failure is a returned reason instead of a
//! child process exit status.
//!
//! The report this writes is the canonical one: the same shape the Playwright
//! reporter emits, so `analyze` treats every surface alike.
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

mod runner;

pub use runner::*;
