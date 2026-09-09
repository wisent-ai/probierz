//! Durable evidence, signing, publication, retention, and the Stado bridge,
//! moved verbatim out of `main.rs`.

use std::path::PathBuf;

use clap::Subcommand;

use crate::stado;

#[derive(Debug, Subcommand)]
pub enum EvidenceCommand {
    // PortEvidence: durable evidence, signing, publication, and retention
    /// Encrypt and authenticate one run's evidence artifacts.
    Protect {
        app_id: Option<String>,
        run_id: Option<String>,
        kind: Option<String>,
        #[arg(long)]
        key_file: Option<PathBuf>,
        #[arg(long = "remove-source")]
        remove_source: bool,
    },
    /// Authenticate and restore an encrypted evidence bundle.
    Restore {
        bundle: Option<PathBuf>,
        destination: Option<PathBuf>,
        #[arg(long)]
        key_file: Option<PathBuf>,
    },
    /// Plan or apply application evidence retention.
    Retention {
        app_id: Option<String>,
        #[arg(long)]
        at: Option<String>,
        #[arg(long)]
        apply: bool,
    },
    /// Find credentials and tokens in an evidence directory.
    SecretScan { directory: Option<PathBuf> },
    /// Query the tamper-evident access audit.
    Audit {
        app_id: Option<String>,
        #[arg(long = "run")]
        run_id: Option<String>,
        #[arg(long)]
        action: Option<String>,
        #[arg(long, default_value = "200")]
        limit: String,
    },
    /// Compare two recorded runs.
    Compare {
        left_run_id: Option<String>,
        right_run_id: Option<String>,
        app_id: Option<String>,
    },
    /// Find the newest passing run.
    LastGreen {
        app_id: Option<String>,
        target: Option<String>,
        journey: Option<String>,
    },
    /// Sign exact runs and policy into an evidence receipt.
    Receipt {
        app_id: Option<String>,
        release: Option<String>,
        expected_harness_sha: Option<String>,
        #[arg(long = "source-sha")]
        expected_source_sha: Option<String>,
        #[arg(long)]
        runs: Option<String>,
        #[arg(long)]
        journeys: Option<String>,
        #[arg(long, default_value = "E3")]
        minimum: String,
    },
    /// Verify a receipt signature, payload hash, and trust anchor.
    VerifyReceipt {
        file: Option<PathBuf>,
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        fingerprint: Option<String>,
    },
    /// Emit a verified immutable first-use publication manifest.
    Publication {
        receipt: Option<PathBuf>,
        attempt_id: Option<String>,
        journey_id: Option<String>,
        #[arg(long)]
        assets: Option<PathBuf>,
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        fingerprint: Option<String>,
    },
    /// Emit an Echo-ingestible onboarding proof manifest.
    PublishOnboarding {
        receipt: Option<PathBuf>,
        #[arg(long = "run")]
        run_id: Option<String>,
        #[arg(long = "journey")]
        journey_id: Option<String>,
        #[arg(long = "journey-version")]
        journey_version: Option<String>,
        #[arg(long = "journey-version-id")]
        journey_version_id: Option<String>,
        #[arg(long = "first-success-fact")]
        first_success_fact: Option<String>,
        #[arg(long = "screen")]
        screen_id: Option<String>,
        #[arg(long)]
        assets: Option<PathBuf>,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        fingerprint: Option<String>,
    },
    // PortStado: remote Stado bridge
    /// Submit, recover, resume, cancel, or author work on the Stado fleet.
    Stado {
        #[command(subcommand)]
        command: stado::StadoCommand,
    },
}
