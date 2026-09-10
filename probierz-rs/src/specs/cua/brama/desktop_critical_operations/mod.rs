//! The Brama desktop critical-operations journey: the fixture it runs
//! against, and the journey driven through the application's own windows.
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

pub(crate) use fixture::*;
pub(crate) use journey::*;
