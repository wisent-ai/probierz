//! The Brama desktop critical-operations journey: the fixture it runs
//! against, and the journey driven through the application's own windows.
//!
//! | part | what it owns |
//! |---|---|
//! | `fixture` | the isolated state directory, Keychain namespace and runtime |
//! | `journey` | launching the app and performing the four operations in order |
//! | `subscriptions` | adding, replacing and removing the provider key |
//! | `routing` | creating, rewriting and deleting the route alias |
//!
//! Every part opens with `use super::*;`, so the list below is the
//! journey's single import list.

pub(crate) use std::collections::BTreeMap;
pub(crate) use std::fs;
pub(crate) use std::net::TcpListener;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::{Command, Stdio};
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant};

pub(crate) use serde_json::Value;

pub(crate) use crate::{cua, specs};

pub(crate) use crate::specs::cua::common;

mod fixture;
mod journey;
pub(crate) mod routing;
pub(crate) mod subscriptions;

pub(crate) use fixture::*;
pub(crate) use journey::*;

/// How long the application may take to settle after being brought to
/// the front before its first window is read.
pub(crate) const LAUNCH_SETTLE: Duration = Duration::from_millis(1500);

/// How long the first window may take to appear after launch.
pub(crate) const WINDOW_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a screen may take to render, or a write to land, after the
/// journey pressed something that changes real state.
pub(crate) const SCREEN_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a dialog may take to appear once opened.
pub(crate) const DIALOG_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a within-screen step may take — pressing a row, opening an
/// inspector — where nothing external is contacted.
pub(crate) const STEP_TIMEOUT: Duration = Duration::from_secs(15);
