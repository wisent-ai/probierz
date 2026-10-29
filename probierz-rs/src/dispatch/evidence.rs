//! Durable evidence, signing, publication, retention and the Stado bridge,
//! dispatched exactly as `main.rs` dispatched them.

use std::path::Path;

use crate::cli::evidence::{EvidenceCommand, ReceiptCommand, SecretsCommand};
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
        EvidenceCommand::Retention {
            app_id,
            at,
            apply,
            fleet,
        } => {
            if fleet {
                evidence::fleet_retention(harness, app_id.as_deref(), at.as_deref(), apply)
            } else {
                evidence::retention(harness, app_id.as_deref(), at.as_deref(), apply)
            }
        }
        EvidenceCommand::Secrets {
            command: SecretsCommand::Scan { directory },
        } => evidence::secrets_scan_command(directory.as_deref()),
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
            limit.as_deref(),
        ),
        EvidenceCommand::Compare {
            left_run_id,
            right_run_id,
            app_id,
        } => evidence::compare(harness, Some(&left_run_id), Some(&right_run_id), &app_id),
        EvidenceCommand::Receipt { command } => match command {
            ReceiptCommand::Create {
                app_id,
                release,
                expected_harness_sha,
                expected_source_sha,
                runs,
                journeys,
                minimum,
            } => evidence::receipt(
                harness,
                evidence::ReceiptRequest {
                    app_id: app_id.as_deref(),
                    release: release.as_deref(),
                    expected_harness: expected_harness_sha.as_deref(),
                    expected_source: expected_source_sha.as_deref(),
                    runs_csv: runs.as_deref(),
                    journeys_csv: journeys.as_deref(),
                    minimum: minimum.as_deref(),
                },
            ),
            ReceiptCommand::Verify {
                file,
                public_key,
                fingerprint,
            } => evidence::verify_receipt(
                file.as_deref(),
                public_key.as_deref(),
                fingerprint.as_deref(),
            ),
        },
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
        // PortStado: remote fleet bridge
        EvidenceCommand::Remote { command } => stado::dispatch(harness, command),
    }
}
