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
        /// First run to compare.
        left_run_id: String,
        /// Second run to compare.
        right_run_id: String,
        /// Application both runs belong to; no application is assumed.
        app_id: String,
    },
    /// Find the newest passing run.
    LastGreen {
        /// Application whose runs are searched; no application is assumed.
        app_id: String,
        /// Only runs on this target.
        #[arg(long)]
        target: Option<String>,
        /// Only runs of this journey.
        #[arg(long)]
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
        /// Receipt file to verify (required).
        file: Option<PathBuf>,
        /// Trusted Ed25519 public key file; without it the key in the
        /// receipt is used and must match `--fingerprint`.
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        /// Expected SHA-256 fingerprint of the signing key; falls back to
        /// PROBIERZ_RECEIPT_PUBLIC_KEY_FINGERPRINT.
        #[arg(long)]
        fingerprint: Option<String>,
    },
    /// Emit a verified immutable first-use publication manifest.
    Publication {
        /// Signed evidence receipt the publication rests on.
        receipt: Option<PathBuf>,
        /// Attempt to publish.
        attempt_id: Option<String>,
        /// Journey the attempt ran.
        journey_id: Option<String>,
        /// Directory of the attempt's published assets.
        #[arg(long)]
        assets: Option<PathBuf>,
        /// Trusted public key that verifies the receipt.
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        /// Expected SHA-256 fingerprint of the receipt's signing key.
        #[arg(long)]
        fingerprint: Option<String>,
    },
    /// Emit an Echo-ingestible onboarding proof manifest.
    PublishOnboarding {
        /// Signed evidence receipt the proof rests on.
        receipt: Option<PathBuf>,
        /// Run whose onboarding is published.
        #[arg(long = "run")]
        run_id: Option<String>,
        /// Onboarding journey the run proves.
        #[arg(long = "journey")]
        journey_id: Option<String>,
        /// Version of that journey.
        #[arg(long = "journey-version")]
        journey_version: Option<String>,
        /// ID of that journey version.
        #[arg(long = "journey-version-id")]
        journey_version_id: Option<String>,
        /// The fact the journey's first success established.
        #[arg(long = "first-success-fact")]
        first_success_fact: Option<String>,
        /// Screen the proof shows.
        #[arg(long = "screen")]
        screen_id: Option<String>,
        /// Directory of the run's published assets.
        #[arg(long)]
        assets: Option<PathBuf>,
        /// File the manifest is written to.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Trusted public key that verifies the receipt.
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        /// Expected SHA-256 fingerprint of the receipt's signing key.
        #[arg(long)]
        fingerprint: Option<String>,
    },
    // PortStado: remote fleet bridge; Stado is the adapter behind it.
    /// Submit, recover, resume, cancel, or author work on a remote fleet host.
    Remote {
        #[command(subcommand)]
        command: stado::StadoCommand,
    },
}
