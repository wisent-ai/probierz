//! Probierz stdio MCP server, `probierz mcp`.
//!
//! Requests are newline-delimited JSON-RPC. Discovery is served without side
//! effects; mutating tools run only after an explicit `tools/call` request.
//! | part | what it owns |
//! |---|---|
//! | `catalog` | the tool catalogue this server advertises |
//! | `jobs` | the runs a mutating tool starts, and the control that owns them |
//! | `protocol` | the JSON-RPC wire, the routes a tool call becomes, and the dispatch |
//!
//! A tool call is the same command the CLI runs, routed to its argv and run
//! in this process through [`crate::run_in_process`]; the server is the
//! product binary, not a second program that shells out to it. Only an
//! asynchronous run (`probierz_start_run`) is a child process, because its
//! whole process tree has to be cancellable.
//!
//! Every part opens with `use crate::mcp::*;`, so the list below is the
//! server's single import list.

pub(crate) use std::collections::HashMap;
pub(crate) use std::fs;
pub(crate) use std::io::{self, BufRead, Read, Write};
pub(crate) use std::path::{Component, Path, PathBuf};
pub(crate) use std::process::{Command, ExitStatus, Stdio};
pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use std::thread;

pub(crate) use base64::engine::general_purpose::STANDARD as BASE64;
pub(crate) use base64::Engine;
pub(crate) use rand_core::{OsRng, RngCore};
pub(crate) use serde_json::{Map, Value};

pub(crate) use crate::failure::now_iso;

mod catalog;
mod jobs;
mod protocol;

pub(crate) use catalog::*;
pub(crate) use jobs::*;
pub(crate) use protocol::*;

/// The harness the server was started for; every tool call reads it.
static HARNESS: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub(crate) fn harness_root() -> PathBuf {
    HARNESS
        .get()
        .cloned()
        .expect("the MCP server is started with a resolved harness")
}

/// Serve the MCP protocol on stdio for `harness` until the client closes it.
pub(crate) fn serve(harness: &Path) {
    let _ = HARNESS.set(harness.to_path_buf());
    protocol::serve();
}
