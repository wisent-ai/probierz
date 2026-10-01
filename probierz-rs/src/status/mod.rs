//! Run history, operator status, dashboards, and desktop failure intake.
//!
//! These are read surfaces over manifests and immutable run manifests. The
//! intake listener is the one write surface here: it appends bounded JSON lines
//! outside TCC-protected project directories so desktop applications and this
//! CLI share one store.

//!
//! The module is read top to bottom: records below a root, the history and
//! dashboard over them, what changed and the gate it must pass, one
//! application's status, the fleet overview, the stored failures, and the
//! intake that stores them.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use base64::Engine;
use chrono::{SecondsFormat, Utc};
use rand_core::RngCore;
use serde_json::{json, Number, Value};

use crate::failure::{print_json, Answer, Code, Failure};
use crate::manifest;

mod application;
mod fleet;
mod intake;
mod runs;

use application::*;
use fleet::*;
use intake::*;
use runs::*;

pub use application::report::status;
pub use fleet::failures::failures;
pub use fleet::overview::overview;
pub use intake::listener::intake_serve;
pub use runs::dashboard::dashboard;
pub use runs::history::history;
pub(crate) use runs::history::run_history_value;
pub(crate) use runs::records::failure_class;

