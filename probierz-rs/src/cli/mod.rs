//! The command surface: one enum per domain, flattened into one parser.
//!
//! `main.rs` carried all of it, 932 lines of it, and this workspace refuses
//! every edit to a file past three hundred lines — so the surface could not be
//! extended at all. Splitting it is what makes it changeable again. `flatten`
//! keeps every command name, every flag and the order they are listed in
//! exactly as they were; the variants below are the same variants, moved.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

pub mod evidence;
pub mod inspect;
pub mod reporting;

#[derive(Debug, Parser)]
#[command(
    name = "probierz",
    about = "Proof that your software works as intended",
    version,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// The harness root holding `apps/`, `packages/` and `test-results/`.
    /// Defaults to the repository this binary was built in, then the working
    /// directory.
    #[arg(long, global = true, value_name = "DIR")]
    pub harness: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Discovery, adoption, authoring and evaluation.
    #[command(flatten)]
    Inspect(inspect::InspectCommand),
    /// Projections over evidence, the incident register, gates and execution.
    #[command(flatten)]
    Reporting(reporting::ReportingCommand),
    /// Durable evidence, signing, publication, retention and the Stado bridge.
    #[command(flatten)]
    Evidence(evidence::EvidenceCommand),
}
