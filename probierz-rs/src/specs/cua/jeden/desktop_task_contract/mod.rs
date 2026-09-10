//! The Jeden desktop task-contract journey, in parts: what it records, how
//! it observes the window, the steps it drives, the recorded run it reads
//! back, and the report it asserts on.
//!
//! Every part opens with `use super::*;`, so the list below is the journey's
//! single import list.

pub(crate) use std::collections::{BTreeMap, HashSet};
pub(crate) use std::fs;
pub(crate) use std::io::{BufRead, BufReader, Write};
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::{Child, Command, Stdio};
pub(crate) use std::sync::{mpsc, Arc, Mutex};
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) use regex::Regex;
pub(crate) use serde_json::Value;

pub(crate) use crate::{
    cua::{self, App, Driver, Snapshot},
    specs,
};

pub(crate) use crate::specs::cua::common;

mod observe;
mod records;
mod recorded;
mod report;
mod steps;

pub(crate) use observe::*;
pub(crate) use records::*;
pub(crate) use recorded::*;
pub(crate) use report::*;
pub(crate) use steps::*;
