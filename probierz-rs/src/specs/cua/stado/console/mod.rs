//! The Stado console as these journeys see it: the view read out of a
//! snapshot, the screen actions taken on it, and the text rules both use.
//!
//! Every part opens with `use super::*;`, so the list below is the
//! module's single import list.

pub(crate) use std::collections::HashMap;
pub(crate) use std::fs;
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant};

pub(crate) use regex::Regex;
pub(crate) use serde_json::Value;

pub(crate) use crate::cua::{self, App, Driver, Snapshot};
pub(crate) use crate::specs;

pub(crate) use crate::specs::cua::common;

mod screen;
mod text;
mod view;

pub(crate) use screen::*;
pub(crate) use text::*;
pub(crate) use view::*;
