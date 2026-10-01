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
        /// Application whose run is protected (required).
        app_id: Option<String>,
        /// Run to protect (required).
        run_id: Option<String>,
        /// Retention kind whose period the bundle carries; the run's own
        /// kind, else `adhoc`, when omitted.
        kind: Option<String>,
        /// File holding the encryption key.
        #[arg(long)]
        key_file: Option<PathBuf>,
        /// Delete the plaintext artifacts once the bundle is written.
        #[arg(long = "remove-source")]
        remove_source: bool,
    },
    /// Authenticate and restore an encrypted evidence bundle.
    Restore {
        /// Encrypted bundle to restore.
        bundle: Option<PathBuf>,
        /// Directory the artifacts are restored into.
        destination: Option<PathBuf>,
        /// File holding the decryption key.
        #[arg(long)]
        key_file: Option<PathBuf>,
    },
    /// Plan or apply application evidence retention.
    Retention {
        /// Application whose evidence is planned (required).
        app_id: Option<String>,
        /// RFC 3339 time expiry is judged at; now when omitted.
        #[arg(long)]
        at: Option<String>,
        /// Delete what has expired instead of only listing it.
        #[arg(long)]
        apply: bool,
        /// Read the evidence this harness left in the fleet's object store
        /// instead of the local test-results tree.
        #[arg(long)]
        fleet: bool,
    },
    /// Find credentials and tokens in an evidence directory.
    SecretScan {
        /// Evidence directory to scan.
        directory: Option<PathBuf>,
    },
    /// Query the tamper-evident access audit.
    Audit {
        /// Only records of this application.
        app_id: Option<String>,
        /// Only records of this run.
        #[arg(long = "run")]
        run_id: Option<String>,
        /// Only records of this action, for example `artifact.protect`.
        #[arg(long)]
        action: Option<String>,
        /// Most records to print; a positive number.
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
        /// Application the receipt is for (required).
        app_id: Option<String>,
        /// Release the receipt is for (required).
        release: Option<String>,
        /// Lowercase SHA-256 of the harness the runs used (required).
        expected_harness_sha: Option<String>,
        /// Lowercase SHA-256 of the product source the runs used (required).
        #[arg(long = "source-sha")]
        expected_source_sha: Option<String>,
        /// Comma-separated run IDs to sign; at least one.
        #[arg(long)]
        runs: Option<String>,
        /// Comma-separated journeys the signed runs must cover.
        #[arg(long)]
        journeys: Option<String>,
        /// Lowest evidence level accepted, `E0` to `E5`.
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
