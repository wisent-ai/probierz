//! The Stado fleet bridge.
//!
//! Submissions are source-bound, credentials remain vault references, and every
//! control-plane answer is retained before it is interpreted.  Remote workers
//! run this Rust binary from the submitted harness rather than a JavaScript
//! compatibility layer.
//!
//! | part | what it owns |
//! |---|---|
//! | [`records`] | the command arguments and the shapes a submission is described by |
//! | [`dispatch`] | which command an operator asked for, and the answer it returns |
//! | [`provision`] | which host a run is placed on, and the shell it is reached through |
//! | [`packing`] | the source packed and uploaded, and the budget a run is given |
//! | [`submit`] | the remote script, the submission, and the watch over the job |
//! | [`collect`] | fetching the evidence, the logs, and cancelling or resuming a run |
//! | [`author`] | remote authoring: its inputs, its receipt, its restore and its submission |
//! | [`byk`] | the iOS login journey's host, bridge and worker |
//! | [`sources`] | the source file list a submission is bound to |
//!
//! Every part opens with `use crate::stado::*;`, so the list below is the
//! module's single import list.

pub(crate) use std::collections::BTreeMap;
pub(crate) use std::fs::{self, File, OpenOptions};
pub(crate) use std::io::{Read, Write};
pub(crate) use std::net::{Shutdown, TcpListener, TcpStream};
pub(crate) use std::os::unix::fs::{FileTypeExt, PermissionsExt};
pub(crate) use std::os::unix::net::{UnixListener, UnixStream};
pub(crate) use std::os::unix::process::ExitStatusExt;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::{Command, Stdio};
pub(crate) use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
pub(crate) use std::sync::Arc;
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) use chrono::{DateTime, Utc};
pub(crate) use clap::{Args, Subcommand};
pub(crate) use serde::Deserialize;
pub(crate) use serde_json::{Map, Value};
pub(crate) use sha2::{Digest, Sha256};
pub(crate) use subtle::ConstantTimeEq;

pub(crate) use crate::discovery;
pub(crate) use crate::failure::{print_json, Answer, Code, Failure};
pub(crate) use crate::manifest;

pub(crate) const STADO_BIN: &str = "stado";
pub(crate) const NODE_VERSION: &str = "v22.20.0";
pub(crate) const UPLOAD_ATTEMPTS: usize = 6;
pub(crate) const UPLOAD_BACKOFF: Duration = Duration::from_secs(5);
pub(crate) const WATCH_INTERVAL: Duration = Duration::from_secs(30);
pub(crate) const STATUS_TIMEOUT: Duration = Duration::from_secs(180);
pub(crate) const GUI_STATUS_TIMEOUT: Duration = Duration::from_secs(1800);
pub(crate) const STATUS_FAILURE_TOLERANCE: usize = 3;
pub(crate) const SETUP_STEP_TIMEOUT_MS: u64 = 30 * 60 * 1000;
pub(crate) const WATCH_BUDGET_ENV: &str = "PROBIERZ_WATCH_BUDGET_MS";
pub(crate) const STADO_RETRY_EXIT: i32 = 69;

pub(crate) const MODEL_ROUTER_REFERENCE: &str = "vault://wisent/probierz/model-router-token";
pub(crate) const MODEL_AGENT_REFERENCE: &str = "vault://wisent/probierz/model-agent-secret";
pub(crate) const SEO_KEY_REFERENCE: &str = "vault://wisent/probierz/seo-receipt-private-key";

mod author;
mod byk;
mod collect;
mod dispatch;
mod packing;
mod provision;
mod records;
mod sources;
mod submit;

pub use dispatch::*;
pub use records::*;
pub use sources::*;
pub(crate) use author::*;
pub(crate) use byk::*;
pub(crate) use collect::*;
pub(crate) use packing::*;
pub(crate) use provision::*;
pub(crate) use submit::*;
