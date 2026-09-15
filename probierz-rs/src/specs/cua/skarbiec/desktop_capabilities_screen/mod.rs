//! Skarbiec Desktop's Capabilities screen, read against a real vault
//! and a real route table the fixture built with the Skarbiec CLI.
//!
//! | part | what it owns |
//! |---|---|
//! | `fixture` | the vault, the routes, and the table's state before |
//! | `journey` | launching the app and opening the screen |
//! | `routes` | what the loaded table must say about each route |
//! | `add` | an add refused without a reason, and that nothing moved |
//!
//! Every part opens with `use super::*;`, so the list below is the
//! journey's single import list.

pub(crate) use std::collections::{BTreeMap, HashMap, HashSet};
pub(crate) use std::fs;
pub(crate) use std::path::PathBuf;
pub(crate) use std::process::Command;
pub(crate) use std::time::Duration;

pub(crate) use serde_json::Value;
pub(crate) use sha2::{Digest, Sha256};

pub(crate) use crate::{cua, specs};

pub(crate) use crate::specs::cua::common;

mod add;
mod fixture;
mod journey;
mod routes;

pub(crate) use add::*;
pub(crate) use fixture::*;
pub(crate) use journey::*;
pub(crate) use routes::*;

/// The three routes the fixture writes, and how the screen must
/// resolve each: one that resolves, one whose item lacks the named
/// field, and one whose item this host cannot read at all.
pub(crate) const FIXTURE_ROUTES: [(&str, &str, &str, &str); 3] = [
    (
        "https://login.example.com",
        "example-login",
        "password",
        "Resolves",
    ),
    (
        "https://sso.example.com",
        "example-login",
        "totp",
        "Field missing",
    ),
    (
        "https://absent.example.com",
        "missing-login",
        "password",
        "Item unreadable",
    ),
];

/// How long the application may take to settle after being brought to
/// the front before its first window is read.
pub(crate) const LAUNCH_SETTLE: Duration = Duration::from_millis(1500);

/// How long the first window may take to appear.
pub(crate) const WINDOW_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the screen may take to render a table it verified route by
/// route against the real vault.
pub(crate) const VERIFIED_TABLE_TIMEOUT: Duration = Duration::from_secs(90);

/// How long a within-screen step may take.
pub(crate) const STEP_TIMEOUT: Duration = Duration::from_secs(15);

/// How long to wait after invoking a refused action before reading the
/// screen again, so a refusal that takes a moment is still observed.
pub(crate) const REFUSAL_SETTLE: Duration = Duration::from_millis(2500);
