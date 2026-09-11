//! What the journey leaves: the proof that no routes command or file carries
//! the secret, and the trace that records the observation and its contracts.

use std::fs;

use serde_json::{json, Value};

use crate::specs::tui::skarbiec::fixture;

use super::journey::{Journey, SECRET_VALUE};

pub(super) struct Source<'a> {
    pub(super) root: &'a str,
    pub(super) revision: &'a str,
    pub(super) dirty: bool,
}

pub(super) struct Outcome {
    pub(super) sound_verify: Value,
    pub(super) broken_report: Value,
    pub(super) backup: String,
}

/// Checks the routes phase of the shell log and the three files for the secret,
/// writes the trace, and closes the shell.
pub(super) fn finish(
    mut journey: Journey<'_>,
    routes_phase_start: usize,
    source: Source<'_>,
    outcome: Outcome,
) -> Result<(), String> {
    let phase_log = journey.shell.full_log();
    let phase_log = phase_log.get(routes_phase_start..).unwrap_or(&phase_log);
    fixture::ensure(
        !phase_log.contains(SECRET_VALUE),
        "a capability routes command emitted the secret its route points at",
    )?;
    for path in [
        &journey.routes_table,
        &journey.beside_journal,
        &journey.routes_audit_file,
    ] {
        let contents =
            fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        fixture::ensure(
            !contents.contains(SECRET_VALUE),
            format!("{} carries secret material", path.display()),
        )?;
    }

    fixture::write_trace(
        journey.context,
        "skarbiec-manage-service-routes.trace.json",
        json!({
            "schemaVersion": 1,
            "kind": "probierz-skarbiec-manage-service-routes-trace",
            "journey": "manage-service-routes",
            "runId": journey.context.optional("PROBIERZ_RUN_ID"),
            "status": "completed",
            "observation": {
                "sourceRoot": source.root,
                "sourceRevision": source.revision,
                "sourceDirty": source.dirty,
                "binary": journey.binary,
                "routesTable": journey.routes_table,
                "commands": journey.executed,
                "soundVerify": outcome.sound_verify,
                "brokenVerify": outcome.broken_report,
                "retainedBackup": outcome.backup,
            },
            "contracts": [
                "routes add without --reason exits non-zero and leaves the table, both journals, and the backup series untouched",
                "routes add with --reason reports the added resource, item, and field and retains the previous table as the backup path it names",
                "a second routes add leaves the existing route untouched, and repeating one reports added=false with no backup and no mutation",
                "routes list reports every route with its item, its field, and whether the vault holds that item and that field",
                "routes verify exits zero with no broken entries on a sound table",
                "routes verify exits non-zero on a broken table and names each broken resource and its problem on stdout",
                "no capability routes command emits the credential its routes point at"
            ],
            "redaction": {"status":"verified_redacted","credentialsIncluded":false,"privateRecordsIncluded":false},
            "publicationRequirements": {"artifactKind":"trace","minimumEvidence":"E2","redactionStatus":"verified_redacted"}
        }),
    )?;
    journey.shell.close()
}
