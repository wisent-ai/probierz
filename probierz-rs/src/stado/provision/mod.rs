//! Where a run is placed and how that host is reached: the selection, and
//! the shell every remote command travels through.

mod select;
mod shell;

pub(crate) use select::*;
pub(crate) use shell::*;
