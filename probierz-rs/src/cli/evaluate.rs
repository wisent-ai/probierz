//! `probierz evaluate`: the release evaluators, one leaf each.

use std::path::PathBuf;

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum EvaluateCommand {
    /// Render and rubric-score a scientific figure pair.
    Figure {
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
    Seo {
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
    /// Judge a rendered talk's slides and speaker script against the documents it presents.
    Talk {
        /// The directory of slide-<number>.png pictures `lem-cli talk render --slides` writes.
        #[arg(long)]
        slides: PathBuf,
        /// The speaker script Markdown `lem-cli talk render --notes` writes.
        #[arg(long)]
        script: PathBuf,
        /// A document the talk must stay true to (Markdown, text, TeX or PDF); repeat for each.
        #[arg(long = "source")]
        sources: Vec<PathBuf>,
        #[arg(long)]
        rubric: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long = "out")]
        output: Option<PathBuf>,
        #[arg(long)]
        router_url: Option<String>,
        #[arg(long)]
        agent_id: Option<String>,
        #[arg(long)]
        router_token_stdin: bool,
    },
}
