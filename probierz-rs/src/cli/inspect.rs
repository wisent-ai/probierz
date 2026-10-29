//! Discovery, adoption, authoring and evaluation commands, moved verbatim out
//! of `main.rs`.

use std::path::PathBuf;

use clap::Subcommand;

use crate::{adoption, apphook_help};

pub use super::evaluate::EvaluateCommand;

#[derive(Debug, Subcommand)]
pub enum InspectCommand {
    // Restored: adoption
    /// Show the first-run journey and optionally adopt existing definitions.
    #[command(
        after_help = adoption::ONBOARDING_HELP,
        override_usage = "probierz onboarding [--reset] [--source <repository>] [--replace] [--json]"
    )]
    Onboarding {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        args: Vec<String>,
    },
    /// Durable project-adoption operations.
    #[command(after_help = adoption::PROJECT_HELP)]
    Project {
        #[command(subcommand)]
        command: adoption::ProjectCommand,
    },
    /// Every test surface, its tool, its npm script and its target coordinates.
    List,
    /// Registered products, their targets and their journeys.
    Apps,
    /// One validated product manifest.
    App { app_id: String },
    /// Run one built-in application setup, broker, or evaluation capability.
    #[command(after_help = apphook_help!())]
    Apphook {
        capability: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// The spec files on disk, optionally for one surface.
    Specs { surface: Option<String> },
    /// The static outline of one spec file: its describe and it titles.
    Describe { spec: String },
    /// The exact shell command that runs a target, printed and not run.
    Cmd { target: String },
    /// The run hosts this harness can use: local and the Stado providers.
    Hosts,
    /// Read a retained security operation journal without running any checks.
    Journal { path: PathBuf },
    // PortAuthoring: authoring, evaluation, and identity
    /// Exact path-independent harness and application source identity.
    Identity { app_id: String },
    /// Validate stable identifiers and native selectors.
    Accessibility { app_id: String },
    /// Draft, execute and accept one journey specification, or draft and
    /// validate a whole application manifest.
    Author {
        #[command(subcommand)]
        command: AuthorCommand,
    },
    /// Dispatch a bounded repair worker for a recorded failed run.
    Repair {
        app_id: String,
        #[arg(long = "run")]
        run_id: Option<String>,
        #[arg(long)]
        rounds: u32,
        #[arg(long)]
        dry_run: bool,
    },
    /// Render and rubric-score a scientific figure pair, or crawl and
    /// evaluate a declared SEO contract.
    Evaluate {
        #[command(subcommand)]
        command: EvaluateCommand,
    },
    /// Render a silent journey video as a looping README GIF.
    Gif {
        input: PathBuf,
        #[arg(long = "out")]
        output: PathBuf,
        /// Seconds into the video the GIF starts at; the start of the video when omitted.
        #[arg(long)]
        start: Option<f64>,
        /// Seconds of video to keep; the rest of the clip from --start when omitted.
        #[arg(long)]
        duration: Option<f64>,
        /// GIF frame rate; the source's own when omitted.
        #[arg(long)]
        fps: Option<f64>,
        /// GIF width in pixels; the source's own when omitted.
        #[arg(long)]
        width: Option<f64>,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthorCommand {
    /// Draft, execute, and accept one real journey specification.
    Spec {
        app_id: String,
        journey: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        desc: String,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        app_path: Option<String>,
        #[arg(long = "paths")]
        mapping_paths: Vec<String>,
        #[arg(long)]
        rounds: u32,
        #[arg(long)]
        dry_run: bool,
    },
    /// Draft and validate a complete application manifest.
    Manifest {
        app_id: String,
        #[arg(long)]
        desc: String,
        #[arg(long)]
        target: String,
        #[arg(long = "repo", required = true)]
        repositories: Vec<String>,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        app_path: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long = "specs")]
        with_specs: bool,
    },
}
