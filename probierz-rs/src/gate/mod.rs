//! Merge and release gates.
//!
//! A verdict names the exact harness, application source, builds, journeys and
//! artifacts it judged.  A gate that cannot prove one of those identities is
//! blocked rather than weakened.
//!
//! | part | what it owns |
//! |---|---|
//! | [`config`] | the gate's arguments, its stored configuration and the readers around both |
//! | [`flags`] | the one read of a yes-or-no evidence field: JSON `true` and nothing else |
//! | [`verdict`] | the evidence a gate reads, the decision it makes, and the commands that carry it |
//! | [`audit`] | the access record every evaluation leaves behind |
//! | [`prepush`] | the pre-push gate and the hook that runs it |
//!
//! One module, kept in parts small enough to read. Every part opens with
//! `use crate::gate::*;`, so the list below is the module's single import
//! list and a part sees the items of every other part exactly as it did when
//! this was one file.

pub(crate) use std::collections::{BTreeSet, HashMap};
pub(crate) use std::fs::{self, OpenOptions};
pub(crate) use std::io::{Read, Write};
pub(crate) use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
pub(crate) use std::path::{Component, Path, PathBuf};
pub(crate) use std::process::{Command as ProcessCommand, Stdio};

pub(crate) use chrono::Utc;
pub(crate) use clap::Args;
pub(crate) use serde_json::{Map, Value};
pub(crate) use serde_yaml::Value as Yaml;

pub(crate) use crate::failure::{print_json, Answer, Failure};
pub(crate) use crate::manifest;

mod audit;
mod config;
mod flags;
mod prepush;
mod verdict;

pub(crate) use audit::*;
pub use config::*;
pub(crate) use flags::*;
pub use prepush::*;
pub use verdict::*;
