//! Application manifests: what a product declares about its journeys.
//!
//! A manifest is the only place a journey's identity, its target coordinates,
//! its retention and its release policy are stated. Every rule below refuses a
//! declaration rather than repairing it, because a manifest that passes while
//! meaning something else is how a release decision gets made about the wrong
//! thing.
//! One module in parts. Every part opens with `use crate::manifest::*;`, so
//! the list below is the module's single import list.

pub(crate) use std::collections::{BTreeMap, BTreeSet};
pub(crate) use std::path::{Path, PathBuf};

pub(crate) use serde::Serialize;
pub(crate) use serde_yaml::Value;

pub(crate) use crate::failure::{Answer, Code, Failure};

mod load;
mod records;
mod validate;

pub use load::*;
pub use records::*;
pub use validate::*;
