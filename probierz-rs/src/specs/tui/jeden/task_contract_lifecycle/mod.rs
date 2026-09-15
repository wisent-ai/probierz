//! Jeden's task contract, end to end: the settings that carry it, the
//! contract a real turn retains, and the delivery report it must
//! produce.
//!
//! | part | what it owns |
//! |---|---|
//! | `contract` | what a retained contract and its delivery report must say |
//! | `turn` | running the binary, its RPC, and one real model turn |
//! | `settings` | the CLI and RPC paths that read and write the contract |
//! | `journey` | the run, the file-tool lifecycle, and the trace |
//!
//! Every part opens with `use super::*;`, so the list below is the
//! journey's single import list.

pub(crate) use crate::specs::{self, tui::common};
pub(crate) use serde_json::{json, Value};
pub(crate) use std::collections::BTreeMap;
pub(crate) use std::fs;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod contract;
mod journey;
mod settings;
mod turn;

pub(crate) use contract::*;
pub(crate) use journey::*;
pub(crate) use settings::*;
pub(crate) use turn::*;

/// The requirements every task contract carries, and therefore exactly
/// the entries every delivery report must carry back.
pub(crate) const REQUIREMENTS: [&str; 7] = [
    "functionality",
    "diagnostics",
    "cli",
    "gui",
    "documentation",
    "tests",
    "delivery",
];

/// How long an ordinary command — a config read or write — may take.
pub(crate) const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);

/// How long the product's own contract test suite, and one real model
/// turn, may take. Both compile or call out to a real model.
pub(crate) const LONG_TIMEOUT: Duration = Duration::from_secs(900);

/// Exit status a refusal carries: the command ran and declined.
pub(crate) const REFUSED_STATUS: i32 = 1;

/// Schema version of the trace this journey writes.
pub(crate) const TRACE_SCHEMA_VERSION: u64 = 1;
