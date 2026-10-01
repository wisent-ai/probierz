//! Running an application-owned journey on the tui, desktop:cua and web
//! surfaces, and the canonical report it writes.
//!
//! Probierz carries no journeys of its own. A journey is a test, it lives in
//! the product's own tree, and it runs only once the operator approved it
//! there; a run names it with `--filter`.
//! | part | what it owns |
//! |---|---|
//! | [`external`] | a journey that is a file in its application's tree |
//! | [`execute`] | running it and timing what it did |
//! | [`report`] | the canonical report written afterwards |
//!
//! Every part opens with `use crate::specs::*;`, so the list below is the
//! module's single import list.

pub(crate) use std::collections::BTreeMap;
pub(crate) use std::fs;
pub(crate) use std::io::Write;
pub(crate) use std::os::unix::fs::PermissionsExt;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::Command;
pub(crate) use std::time::{Duration, Instant, SystemTime};

pub(crate) use serde_json::Value;

pub(crate) use crate::failure::{create_private, fail, iso_timestamp, Failure};

mod runner;

pub use runner::*;
