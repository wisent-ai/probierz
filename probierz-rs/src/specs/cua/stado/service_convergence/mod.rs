//! The Stado service-convergence journey: the real fixture it runs
//! against, the Services screen it reads, and the journey itself.
//!
//! | part | what it owns |
//! |---|---|
//! | `fixture` | starting the real convergence fixture and waiting on it |
//! | `screen` | opening Services and recognising a convergence receipt |
//! | `journey` | the journey a person would perform, window by window |
//!
//! Every part opens with `use super::*;`, so the list below is the
//! journey's single import list.

pub(crate) use std::collections::BTreeMap;
pub(crate) use std::fs::{self, OpenOptions};
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::{Child, Command, Stdio};
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant};

#[cfg(unix)]
pub(crate) use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

pub(crate) use regex::Regex;
pub(crate) use serde_json::Value;

pub(crate) use crate::{cua::App, specs};

pub(crate) use super::console;

mod fixture;
mod journey;
mod screen;

pub(crate) use fixture::*;
pub(crate) use journey::*;
pub(crate) use screen::*;
