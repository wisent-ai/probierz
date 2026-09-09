//! One command in, one answer out. The arms live beside the enum group they
//! belong to, so a domain's surface and its dispatch move together.

mod evidence;
mod inspect;
mod reporting;

use std::path::Path;

use crate::cli::Command;
use crate::failure::Answer;

pub fn dispatch(harness: &Path, command: Command) -> Answer {
    match command {
        Command::Inspect(command) => inspect::dispatch(harness, command),
        Command::Reporting(command) => reporting::dispatch(harness, command),
        Command::Evidence(command) => evidence::dispatch(harness, command),
    }
}
