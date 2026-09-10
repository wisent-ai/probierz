//! Adopting an existing project: what it already has, what Probierz would
//! add, what would collide, and the first-run journey that walks an operator
//! through the answer.
//!
//! | part | what it owns |
//! |---|---|
//! | [`project`] | the adoption itself, the index behind it, what collides, and the paths and digests it works in |
//! | [`sources`] | the definitions a source tree offers and the index they are recorded in |
//! | [`journey`] | the first-run journey and the progress it keeps |
//!
//! One module, kept in parts small enough to read. Every part opens with
//! `use crate::adoption::*;`, so the list below is the module's single import
//! list and a part sees the items of every other part exactly as it did when
//! this was one file.

pub(crate) use std::collections::{BTreeSet, HashMap};
pub(crate) use std::ffi::OsStr;
pub(crate) use std::fs::{self, OpenOptions};
pub(crate) use std::io::{Read, Write};
pub(crate) use std::path::{Component, Path, PathBuf};
pub(crate) use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) use clap::Subcommand;
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::Value;
pub(crate) use sha2::{Digest, Sha256};

pub(crate) use crate::failure::{fail, now_iso, print_json, write_private, Answer, Failure};

mod journey;
mod project;
mod sources;

pub use journey::*;
pub use project::*;
pub(crate) use sources::*;
