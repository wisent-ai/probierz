//! Execution, preflight, report analysis, affected-target selection, CI orchestration,
//! and declared run matrices.
//!
//! The suite drivers remain the real product tools. This module only prepares
//! their environment, starts `npm run <script>` with the same argument vector as
//! the former Node runner, and turns the reports they write into durable facts.
//!
//! | part | what it owns |
//! |---|---|
//! | [`plan`] | the target table, the arguments a run is asked for, what a machine must already have, and which targets a change selects |
//! | [`session`] | starting a child, reading it, stopping its tree, and what the machine and its media said while it ran |
//! | [`report`] | one report shape out of every driver, the timeline behind it, the verdict read off it, and the identity it is bound to |
//! | [`drive`] | running one surface, the Byk broker an iOS login needs, and the CI and matrix fan-out |

// One module, kept in parts small enough to read. Every part opens with
// `use crate::run::*;`, so the list below is the module's single import list
// and a part sees the items of every other part exactly as it did when this
// was one file.
pub(crate) use std::collections::{BTreeMap, BTreeSet, HashMap};
pub(crate) use std::fs::{self, File, OpenOptions};
pub(crate) use std::io::{BufRead, BufReader, Read, Write};
pub(crate) use std::os::unix::fs::{FileTypeExt, PermissionsExt};
pub(crate) use std::path::{Component, Path, PathBuf};
pub(crate) use std::process::{Child, Command, ExitStatus, Stdio};
pub(crate) use std::sync::{mpsc, Arc, Mutex};
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) use chrono::{DateTime, SecondsFormat, Utc};
pub(crate) use flate2::read::DeflateDecoder;
pub(crate) use regex::Regex;
pub(crate) use serde_json::{Map, Number, Value};
pub(crate) use sha2::{Digest, Sha256};
pub(crate) use url::Url;

pub(crate) use crate::failure::{fail, now_iso, print_json, Answer, Failure};
pub(crate) use crate::manifest;

pub(crate) const TAIL: usize = 4000;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 20 * 60 * 1000;
pub(crate) const PROBE_MS: u64 = 15_000;
pub(crate) const SAMPLE_INTERVAL_MS: u64 = 1000;

mod drive;
mod plan;
pub(crate) mod report;
mod session;

pub(crate) use drive::*;
pub(crate) use plan::*;
pub(crate) use report::*;
pub(crate) use session::*;
