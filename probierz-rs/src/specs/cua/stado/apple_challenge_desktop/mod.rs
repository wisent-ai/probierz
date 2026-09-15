//! Preparing Apple code capture on a dedicated Stado host, driven
//! through Stado Desktop's own windows.
//!
//! | part | what it owns |
//! |---|---|
//! | `readiness` | reading CuaDriver readiness and the product's report |
//! | `hosts` | finding the one Hosts row for the named host |
//! | `journey` | the journey: select, read readiness, prepare, verify |
//!
//! Every part opens with `use super::*;`, so the list below is the
//! journey's single import list.

pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::Command;
pub(crate) use std::time::Duration;

pub(crate) use regex::Regex;
pub(crate) use serde_json::Value;

pub(crate) use crate::specs;

pub(crate) use super::console;

mod hosts;
mod journey;
mod readiness;

pub(crate) use hosts::*;
pub(crate) use journey::*;
pub(crate) use readiness::*;

/// How long a screen this journey reads may take to render.
pub(crate) const GATES: Duration = Duration::from_secs(180);

/// How long a real preparation on the dedicated host may take. It
/// installs or reuses a signed helper and exercises it in the
/// registry-bound Aqua session.
pub(crate) const PREPARATION: Duration = Duration::from_secs(360);

/// The Apple helper version the product must report.
pub(crate) const APPLE_HELPER_VERSION: &str = "2";
