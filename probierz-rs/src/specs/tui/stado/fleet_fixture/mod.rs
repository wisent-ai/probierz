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

/// The Stado checkout whose build this fixture drives.
pub(crate) fn stado_repo() -> PathBuf {
    specs::wisent_checkout("stado")
}

/// The Stado binary built from that checkout, when the run names none.
pub(crate) fn default_stado_binary() -> String {
    stado_repo().join("stado-rs/target/release/stado").to_string_lossy().into_owned()
}
pub(crate) const FIXTURE_HOST: &str = "probierz-fixture-host";
pub(crate) const FIXTURE_PRODUCT: &str = "probierz-fixture-product";

mod agents;
mod documents;
mod registry;

pub(crate) use agents::*;
pub(crate) use documents::*;
pub(crate) use registry::*;
