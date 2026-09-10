//! The isolated fleet the Stado journeys run against: the documents it is
//! built from, the registry and release control it publishes, and the
//! launchd agents it starts and stops.
//!
//! Every part opens with `use super::*;`, so the list below is the
//! fixture's single import list.

pub(crate) use std::fs;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::Command;
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) use regex::Regex;
pub(crate) use serde_json::Value;

pub(crate) use crate::failure::{iso_timestamp, now_iso, write_private};
pub(crate) use crate::{specs, tui};

pub(crate) const STADO_REPO: &str =
    "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/wisent-compute";
pub(crate) const DEFAULT_STADO_BINARY: &str =
    "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/wisent-compute/stado-rs/target/release/stado";
pub(crate) const FIXTURE_HOST: &str = "probierz-fixture-host";
pub(crate) const FIXTURE_PRODUCT: &str = "probierz-fixture-product";

mod agents;
mod documents;
mod registry;

pub(crate) use agents::*;
pub(crate) use documents::*;
pub(crate) use registry::*;
