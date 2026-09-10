//! Product-specific setup and evaluation hooks executed by Probierz itself.
//!
//! Manifests name these capabilities. They are not script paths: the same
//! implementation serves lifecycle runs and the `probierz apphook` command.
//!
//! | part | what it owns |
//! |---|---|
//! | [`entry`] | which capabilities exist, what each one requires, and the command that runs one |
//! | [`http`] | the requests these hooks make and the answers they accept |
//! | [`oko`] | the Oko fixture: its account, its seeded documents and its cleanup |
//! | [`gac`] | the GAC fixtures, the render they produce and the evaluation of it |
//!
//! Every part opens with `use crate::apphooks::*;`, so the list below is the
//! module's single import list.

pub(crate) use std::collections::{BTreeMap, BTreeSet};
pub(crate) use std::fs;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::Command;
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant, SystemTime};

pub(crate) use base64::Engine;
pub(crate) use chrono::{DateTime, SecondsFormat, Utc};
pub(crate) use serde_json::Value;
pub(crate) use sha2::{Digest, Sha256};
pub(crate) use url::Url;

pub(crate) use crate::failure::{print_json, write_private, Answer, Code, Failure};

mod entry;
mod gac;
mod http;
mod oko;

pub use entry::*;
pub(crate) use gac::*;
pub(crate) use http::*;
pub(crate) use oko::*;
