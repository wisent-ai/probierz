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

mod app;
mod changes;
mod dashboard;
mod envelope;
mod failures;
mod gates;
mod history;
mod intake;
mod overview;
mod records;

use app::*;
use changes::*;
use dashboard::*;
use envelope::*;
use failures::*;
use gates::*;
use records::*;

pub use app::status;
pub use dashboard::dashboard;
pub use failures::failures;
pub use history::history;
pub(crate) use history::run_history_value;
pub use intake::intake_serve;
pub use overview::overview;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_has_javascript_star_semantics() {
        assert!(glob_matches("Sources/**/*.swift", "Sources/App/View.swift"));
        assert!(!glob_matches("Sources/*.swift", "Sources/App/View.swift"));
        assert!(glob_matches("Package.swift", "Package.swift"));
    }

    #[test]
    fn failure_point_requires_dotted_lowercase_segments() {
        assert!(valid_failure_point("desktop.login.auth-failed"));
        assert!(!valid_failure_point("Desktop.login"));
        assert!(!valid_failure_point("desktop..login"));
        assert!(!valid_failure_point("desktop.-login"));
    }

    #[test]
    fn service_filename_preserves_existing_hyphens() {
        assert_eq!(service_file_name(" A- B "), "a--b.jsonl");
        assert_eq!(service_file_name("%%%"), "unknown.jsonl");
    }
}
