//! Probierz stdio MCP server.
//!
//! Requests are newline-delimited JSON-RPC. Discovery is served without side
//! effects; mutating tools run only after an explicit `tools/call` request.
//! | part | what it owns |
//! |---|---|
//! | `catalog` | the tool catalogue this server advertises |
//! | `jobs` | the runs a mutating tool starts, and the control that owns them |
//! | `protocol` | the JSON-RPC wire, the routes a tool call becomes, and the dispatch |
//!
//! Every part opens with `use crate::*;`, so the list below is the binary's
//! single import list.

#[allow(dead_code)]
#[path = "../failure.rs"]
pub(crate) mod failure;

pub(crate) use std::collections::HashMap;
pub(crate) use std::fs;
pub(crate) use std::io::{self, BufRead, Read, Write};
pub(crate) use std::path::{Component, Path, PathBuf};
pub(crate) use std::process::{Child, Command, ExitStatus, Stdio};
pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use std::thread;
pub(crate) use std::time::Duration;

pub(crate) use base64::engine::general_purpose::STANDARD as BASE64;
pub(crate) use base64::Engine;
pub(crate) use rand_core::{OsRng, RngCore};
pub(crate) use serde_json::{Map, Value};

pub(crate) use failure::now_iso;

mod catalog;
mod jobs;
mod protocol;

pub(crate) use catalog::*;
pub(crate) use jobs::*;
pub(crate) use protocol::*;

fn main() {
    protocol::serve();
}
