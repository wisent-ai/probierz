//! Publication of a recorded journey as a bounded README GIF.
//!
//! Three parts: the records a publication is described by, the bounds and
//! file checks it must satisfy, and the render itself. Every part opens with
//! `use crate::readme_gif::*;`, so the list below is the module's single
//! import list.

pub(crate) use std::fs::{self, File};
pub(crate) use std::io::Read;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::Command;

pub(crate) use serde::Serialize;
pub(crate) use serde_json::{Number, Value};
pub(crate) use sha2::{Digest, Sha256};

pub(crate) use crate::failure::{print_json, write_private, Answer, Failure};

mod bounds;
mod create;
mod records;

pub use create::*;
pub(crate) use bounds::*;
pub use records::*;
