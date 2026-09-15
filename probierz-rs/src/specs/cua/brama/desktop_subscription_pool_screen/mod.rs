//! Brama Desktop's Subscription Pool screen, read against a real
//! ledger the fixture wrote, through the application's own windows.
//!
//! | part | what it owns |
//! |---|---|
//! | `fixture` | the ledger, the CLI wrapper, and what must never render |
//! | `journey` | launching the app and opening the screen |
//! | `pool` | what the loaded table must say about each subscription |
//! | `refresh` | the inspector, and a refresh refused without a reason |
//!
//! Every part opens with `use super::*;`, so the list below is the
//! journey's single import list.

pub(crate) use std::collections::{BTreeMap, HashSet};
pub(crate) use std::fs;
pub(crate) use std::os::unix::fs::PermissionsExt;
pub(crate) use std::path::PathBuf;
pub(crate) use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) use regex::Regex;
pub(crate) use serde_json::{json, Value};

pub(crate) use crate::{cua, specs};

pub(crate) use crate::specs::cua::common;

mod fixture;
mod journey;
mod pool;
mod refresh;

pub(crate) use fixture::*;
pub(crate) use journey::*;
pub(crate) use pool::*;
pub(crate) use refresh::*;

/// What the fixture's ledger must produce on screen: the subscription
/// identity, its provider, the state the screen derives, and the
/// provider's own refusal where there is one.
pub(crate) const EXPECTED: [(&str, &str, &str, Option<&str>); 4] = [
    ("sub-anthropic-7f21", "anthropic", "Live", None),
    (
        "sub-google-93bd",
        "google",
        "Expired",
        Some("the pooled grant expired before the last dispatch"),
    ),
    ("sub-mistral-4a88", "mistral", "Unknown", None),
    (
        "sub-openai-1c04",
        "openai",
        "Burnt",
        Some("the provider refused the stored grant: seat revoked"),
    ),
];

/// How long the application may take to settle after being brought to
/// the front before its first window is read.
pub(crate) const LAUNCH_SETTLE: Duration = Duration::from_millis(1500);

/// How long the first window, or a screen backed by a real CLI read,
/// may take to appear.
pub(crate) const WINDOW_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a screen backed by state already in the process may take.
pub(crate) const SCREEN_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a dialog or destination may take to open.
pub(crate) const DIALOG_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a within-screen step may take.
pub(crate) const STEP_TIMEOUT: Duration = Duration::from_secs(15);

/// How long to wait after invoking a refused action before reading the
/// screen again, so a refusal that takes a moment is still observed.
pub(crate) const REFUSAL_SETTLE: Duration = Duration::from_millis(2500);
