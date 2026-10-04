//! Discovery, adoption, authoring and evaluation commands, moved verbatim out
//! of `main.rs`.

use std::path::PathBuf;

use clap::Subcommand;

use crate::{adoption, apphook_help};

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
    // PortAuthoring: authoring, evaluation, and identity
    /// Exact path-independent harness and application source identity.
    SourceIdentity { app_id: String },
    /// Validate stable identifiers and native selectors.
    Accessibility { app_id: String },
    /// Draft, execute, and accept one real journey specification.
    AuthorSpec {
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
    AuthorManifest {
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
    /// Render and rubric-score a scientific figure pair.
    FigureEvaluate {
        #[arg(long)]
        reference: PathBuf,
        #[arg(long)]
        candidate: PathBuf,
        #[arg(long)]
        rubric: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long = "out")]
        output: Option<PathBuf>,
        #[arg(long)]
        router_url: Option<String>,
        #[arg(long)]
        tex_preamble: Option<PathBuf>,
        #[arg(long)]
        agent_id: Option<String>,
        #[arg(long)]
        router_token_stdin: bool,
    },
    /// Crawl and evaluate a declared SEO contract.
    SeoEvaluate {
        /// Manifest app whose SEO profile is evaluated; no app is assumed.
        #[arg(long = "app")]
        app_id: String,
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        policy: Option<PathBuf>,
        #[arg(long)]
        brief: Option<PathBuf>,
        /// The SEO profile to evaluate: pull-request, release, nightly or production.
        #[arg(long)]
        mode: String,
        #[arg(long = "out")]
        output: Option<PathBuf>,
        #[arg(long)]
        production_evidence: Option<PathBuf>,
        #[arg(long)]
        primary_model: Option<String>,
        #[arg(long)]
        secondary_model: Option<String>,
        #[arg(long)]
        adjudicator_model: Option<String>,
        #[arg(long)]
        router_url: Option<String>,
        #[arg(long)]
        agent_id: Option<String>,
        #[arg(long)]
        private_key_file: Option<PathBuf>,
        #[arg(long)]
        router_token_stdin: bool,
    },
    // ReadmeGif
    /// Render a silent journey video as a looping README GIF.
    ReadmeGif {
        input: PathBuf,
        #[arg(long = "out")]
        output: PathBuf,
        #[arg(long, default_value_t = 0.0)]
        start: f64,
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
