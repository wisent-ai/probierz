//! Native desktop automation through the `cua-driver` command line service.
//!
//! Targets obtained from an accessibility snapshot are snapshot-bound.  The
//! helpers below therefore keep the snapshot id and element token together and
//! never turn a tree match into an unscoped integer action.
//!
//! | part | what it owns |
//! |---|---|
//! | [`records`] | the driver handle, the app, the snapshot and the bounds they speak in |
//! | [`driver`] | reaching the service, driving a window, and the windows themselves |
//! | [`elements`] | reading one element out of a snapshot tree |
//!
//! Every part opens with `use crate::cua::*;`, so the list below is the
//! module's single import list.

pub(crate) use std::collections::BTreeMap;
pub(crate) use std::fs;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::{Child, Command, Output, Stdio};
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant};

pub(crate) use serde_json::Value;

mod driver;
mod elements;
mod records;

pub use elements::*;
pub use records::*;
