use crate::stado::*;

#[derive(Debug, Subcommand)]
pub enum StadoCommand {
    /// Submit one target to the selected Stado host.
    Run(RunArgs),
    /// Read a job once and collect terminal evidence.
    Collect(CollectArgs),
    /// Continue watching an existing job without resubmitting it.
    Resume(ResumeArgs),
    /// Cancel a job and retain the complete cancellation attempt.
    Cancel(CancelArgs),
    /// Author and verify one journey on a Stado host.
    Author(AuthorArgs),
    /// Run the complete SEO evaluator remotely.
    Seo(SeoArgs),
    /// Internal source-bound receipt writer used only inside an author job.
    #[command(hide = true)]
    AuthorReceipt(AuthorReceiptArgs),
    /// Internal entry point copied to the dedicated iOS worker.
    #[command(hide = true)]
    BykAuthWorker,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    pub target: Option<String>,
    #[arg(long)]
    pub app: Option<String>,
    #[arg(long)]
    pub spec: Option<String>,
    #[arg(long)]
    pub record: bool,
    #[arg(long, default_value = "stado:gcp")]
    pub host: String,
    #[arg(long)]
    pub cargo_release: bool,
    #[arg(long)]
    pub app_repo: Option<PathBuf>,
    #[arg(long)]
    pub binary: Option<String>,
    #[arg(long)]
    pub cargo_manifest: Option<String>,
    #[arg(long, num_args = 0..=1)]
    pub app_binary_path: Option<Option<PathBuf>>,
    #[arg(long, num_args = 0..=1)]
    pub app_bundle_path: Option<Option<PathBuf>>,
    #[arg(long)]
    pub node_source: bool,
    #[arg(long, action = clap::ArgAction::Append)]
    pub env: Vec<String>,
    #[arg(long)]
    pub script: Option<String>,
    #[arg(long)]
    pub no_watch: bool,
}

#[derive(Debug, Args)]
pub struct CollectArgs {
    pub job_id: Option<String>,
    #[arg(long)]
    pub app: Option<String>,
    #[arg(long, default_value = "stado:mini")]
    pub host: String,
}

#[derive(Debug, Args)]
pub struct ResumeArgs {
    pub job_id: Option<String>,
    #[arg(long, default_value = "stado:any")]
    pub host: String,
}

#[derive(Debug, Args)]
pub struct CancelArgs {
    pub job_id: Option<String>,
    #[arg(long)]
    pub host: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Args)]
pub struct AuthorArgs {
    pub app_id: Option<String>,
    pub journey: Option<String>,
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long)]
    pub desc: Option<String>,
    #[arg(long)]
    pub area: Option<String>,
    #[arg(long, default_value = "stado:gcp")]
    pub host: String,
    #[arg(long, num_args = 0..=1)]
    pub app_path: Option<Option<PathBuf>>,
    #[arg(long, num_args = 0..=1)]
    pub app_binary_path: Option<Option<PathBuf>>,
    #[arg(long, num_args = 0..=1)]
    pub app_bundle_path: Option<Option<PathBuf>>,
    #[arg(long)]
    pub app_repo: Option<PathBuf>,
    #[arg(long)]
    pub cargo_release: bool,
    #[arg(long)]
    pub binary: Option<String>,
    #[arg(long)]
    pub cargo_manifest: Option<String>,
    #[arg(long)]
    pub no_watch: bool,
}

#[derive(Debug, Args)]
pub struct SeoArgs {
    pub app_id: Option<String>,
    #[arg(long)]
    pub base_url: Option<String>,
    #[arg(long, default_value = "release")]
    pub mode: String,
    #[arg(long)]
    pub policy: Option<String>,
    #[arg(long)]
    pub brief: Option<String>,
    #[arg(long)]
    pub production_evidence: Option<PathBuf>,
    #[arg(long)]
    pub primary_model: Option<String>,
    #[arg(long)]
    pub secondary_model: Option<String>,
    #[arg(long)]
    pub adjudicator_model: Option<String>,
    #[arg(long, default_value = "probierz")]
    pub agent_id: String,
    #[arg(long, default_value = "stado:mini")]
    pub host: String,
    #[arg(long)]
    pub no_watch: bool,
}

#[derive(Debug, Args)]
pub struct AuthorReceiptArgs {
    #[arg(long)]
    pub app: String,
    #[arg(long)]
    pub journey: String,
    #[arg(long)]
    pub area: String,
    #[arg(long)]
    pub target: String,
    #[arg(long)]
    pub receipt_id: String,
    #[arg(long)]
    pub result: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) enum Provision {
    InstalledTui {
        app_id: String,
        path: PathBuf,
    },
    NativeBinary {
        app_id: String,
        binary_path: PathBuf,
        binary_name: Option<String>,
        binary_sha256: Option<String>,
    },
    CargoRelease {
        app_id: String,
        binary: String,
        manifest_path: String,
    },
    AppBundle {
        app_id: String,
        bundle_path: PathBuf,
        bundle_name: Option<String>,
    },
    NodeSource {
        app_id: String,
        script: Option<String>,
    },
}

impl Provision {
    pub(crate) fn app_id(&self) -> &str {
        match self {
            Self::InstalledTui { app_id, .. }
            | Self::NativeBinary { app_id, .. }
            | Self::CargoRelease { app_id, .. }
            | Self::AppBundle { app_id, .. }
            | Self::NodeSource { app_id, .. } => app_id,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ProcessOutput {
    pub(crate) command: String,
    pub(crate) args: Vec<String>,
    pub(crate) status: Option<i32>,
    pub(crate) signal: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) error: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Packed {
    pub(crate) file: PathBuf,
    pub(crate) hash: String,
}

#[derive(Debug)]
pub(crate) struct Identity {
    pub(crate) document: Value,
    pub(crate) file: PathBuf,
    pub(crate) hash: String,
}

#[derive(Debug)]
pub(crate) struct Submission {
    pub(crate) job_id: Option<String>,
    pub(crate) watch_budget_ms: u64,
    pub(crate) receipt_dir: PathBuf,
    pub(crate) failure: Option<Value>,
}

#[derive(Debug)]
pub(crate) struct Retained {
    pub(crate) results_dir: Option<PathBuf>,
    pub(crate) manifest: Option<Value>,
    pub(crate) author_receipt: Option<Value>,
    pub(crate) author_receipt_file: Option<PathBuf>,
    pub(crate) artifact_error: Option<Value>,
}

