//! Durable evidence, signing, publication, retention and the Stado bridge,
//! dispatched exactly as `main.rs` dispatched them.

use std::path::Path;

use crate::cli::evidence::EvidenceCommand;
use crate::failure::Answer;
use crate::{evidence, stado};

pub fn dispatch(harness: &Path, command: EvidenceCommand) -> Answer {
    match command {
        // PortEvidence: durable evidence, signing, publication, and retention
        EvidenceCommand::Protect {
            app_id,
            run_id,
            kind,
            key_file,
            remove_source,
        } => evidence::protect(
            harness,
            app_id.as_deref(),
            run_id.as_deref(),
            kind.as_deref(),
            key_file.as_deref(),
            remove_source,
        ),
        EvidenceCommand::Restore {
            bundle,
            destination,
            key_file,
        } => evidence::restore(
            harness,
            bundle.as_deref(),
            destination.as_deref(),
            key_file.as_deref(),
        ),
        EvidenceCommand::Retention { app_id, at, apply } => {
            evidence::retention(harness, app_id.as_deref(), at.as_deref(), apply)
        }
        EvidenceCommand::SecretScan { directory } => evidence::secret_scan(directory.as_deref()),
        EvidenceCommand::Audit {
            app_id,
            run_id,
            action,
            limit,
        } => evidence::audit(
            harness,
            app_id.as_deref(),
            run_id.as_deref(),
            action.as_deref(),
            &limit,
        ),
        EvidenceCommand::Compare {
            left_run_id,
            right_run_id,
            app_id,
        } => evidence::compare(
            harness,
            left_run_id.as_deref(),
            right_run_id.as_deref(),
            app_id.as_deref(),
        ),
        EvidenceCommand::LastGreen {
            app_id,
            target,
            journey,
        } => evidence::last_green(
            harness,
            app_id.as_deref(),
            target.as_deref(),
            journey.as_deref(),
        ),
        EvidenceCommand::Receipt {
            app_id,
            release,
            expected_harness_sha,
            expected_source_sha,
            runs,
            journeys,
            minimum,
        } => evidence::receipt(
            harness,
            app_id.as_deref(),
            release.as_deref(),
            expected_harness_sha.as_deref(),
            expected_source_sha.as_deref(),
            runs.as_deref(),
            journeys.as_deref(),
            &minimum,
        ),
        EvidenceCommand::VerifyReceipt {
            file,
            public_key,
            fingerprint,
        } => evidence::verify_receipt(
            file.as_deref(),
            public_key.as_deref(),
            fingerprint.as_deref(),
        ),
        EvidenceCommand::Publication {
            receipt,
            attempt_id,
            journey_id,
            assets,
            public_key,
            fingerprint,
        } => evidence::publication(
            harness,
            receipt.as_deref(),
            attempt_id.as_deref(),
            journey_id.as_deref(),
            assets.as_deref(),
            public_key.as_deref(),
            fingerprint.as_deref(),
        ),
        EvidenceCommand::PublishOnboarding {
            receipt,
            run_id,
            journey_id,
            journey_version,
            journey_version_id,
            first_success_fact,
            screen_id,
            assets,
            output,
            public_key,
            fingerprint,
        } => evidence::publish_onboarding(
            receipt.as_deref(),
            run_id.as_deref(),
            journey_id.as_deref(),
            journey_version.as_deref(),
            journey_version_id.as_deref(),
            first_success_fact.as_deref(),
            screen_id.as_deref(),
            assets.as_deref(),
            output.as_deref(),
            public_key.as_deref(),
            fingerprint.as_deref(),
        ),
        // PortStado: remote Stado bridge
        EvidenceCommand::Stado { command } => stado::dispatch(harness, command),
    }
}
