//! The register of attempts that did not hold.
//!
//! Probierz already records what a run did: `run-manifest.json` binds a result
//! to exact source identity, and `failures` shows the envelopes a running
//! application posted to the intake listener. Neither can hold the third thing
//! that happens: somebody — a person or an agent — claims a piece of work is
//! done, and the claim turns out not to hold. That claim has no run, so it has
//! no manifest, and it never reached a socket, so it has no envelope. It was
//! therefore recorded nowhere, and the same failure could recur with nothing on
//! disk knowing it had happened before.
//!
//! This register is that record. One append-only file,
//! `test-results/.incidents/register.jsonl`, beside the audit the evidence
//! model already describes. Two record kinds live in it, an incident and its
//! resolution; reading folds the second onto the first, so state is derived
//! rather than edited and no line is ever rewritten.
//!
//! The vocabulary is not this module's to invent. An incident carries a
//! `wisent-errors` envelope, the same shape the intake listener stores, and the
//! required fields are refused by name when one is missing.

mod commands;
mod store;

use std::path::{Path, PathBuf};

use clap::Subcommand;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::failure::Answer;

pub const HELP: &str = "\
The register answers three questions: what was claimed, what the claim was
worth, and whether it was ever closed.

  probierz incident record --claim <text> --envelope <file|->
  probierz incident record --claim <text> --service <name> --failure-point <point> --code <code> --detail <text>
  probierz incident list [--state open|resolved|all] [--limit N] [--json]
  probierz incident show <id> [--json]
  probierz incident resolve <id> --note <text> [--run <runId>]

An envelope is a wisent-errors envelope: failure_point, error_code, service
and detail are required, and a missing one is refused by name.";

pub(crate) const INCIDENT_SCHEMA: &str = "ai.wisent.probierz.incident.v1";
pub(crate) const RESOLUTION_SCHEMA: &str = "ai.wisent.probierz.incident-resolution.v1";
const REGISTER_DIRECTORY: &str = "test-results/.incidents";
const REGISTER_FILE: &str = "register.jsonl";
const IDENTITY_HEX: usize = 16;

#[derive(Debug, Subcommand)]
pub enum IncidentCommand {
    /// Record one attempt that did not hold.
    Record {
        /// What was claimed, in the words it was claimed in.
        #[arg(long)]
        claim: String,
        /// A wisent-errors envelope to carry, or `-` to read one from stdin.
        #[arg(long)]
        envelope: Option<String>,
        #[arg(long)]
        service: Option<String>,
        #[arg(long = "failure-point")]
        failure_point: Option<String>,
        #[arg(long = "code")]
        error_code: Option<String>,
        #[arg(long)]
        detail: Option<String>,
        /// The run this incident is about, when one exists.
        #[arg(long = "run")]
        run_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Every recorded incident, newest first.
    List {
        #[arg(long, default_value = "open")]
        state: String,
        #[arg(long, default_value_t = 20, value_parser = crate::cli::reporting::positive_history_limit)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// One incident and its resolution, if it has one.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Close one incident, naming what closed it.
    Resolve {
        id: String,
        #[arg(long)]
        note: String,
        #[arg(long = "run")]
        run_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

pub fn dispatch(harness: &Path, command: IncidentCommand) -> Answer {
    match command {
        IncidentCommand::Record {
            claim,
            envelope,
            service,
            failure_point,
            error_code,
            detail,
            run_id,
            json,
        } => commands::record(
            harness,
            commands::Recorded {
                claim: &claim,
                envelope: envelope.as_deref(),
                service: service.as_deref(),
                failure_point: failure_point.as_deref(),
                error_code: error_code.as_deref(),
                detail: detail.as_deref(),
                run_id: run_id.as_deref(),
            },
            json,
        ),
        IncidentCommand::List { state, limit, json } => {
            commands::list(harness, &state, limit, json)
        }
        IncidentCommand::Show { id, json } => commands::show(harness, &id, json),
        IncidentCommand::Resolve {
            id,
            note,
            run_id,
            json,
        } => commands::resolve(harness, &id, &note, run_id.as_deref(), json),
    }
}

/// Where the register lives, under the harness root every other Probierz
/// record is written beneath.
pub fn register_file(harness: &Path) -> PathBuf {
    harness.join(REGISTER_DIRECTORY).join(REGISTER_FILE)
}

/// Who is recording. The evidence model already attributes audit records to
/// `PROBIERZ_ACTOR`, `GITHUB_ACTOR` or `USER`, so the register attributes them
/// the same way rather than inventing a second answer.
pub(crate) fn actor() -> String {
    for name in ["PROBIERZ_ACTOR", "GITHUB_ACTOR", "USER"] {
        if let Ok(value) = std::env::var(name) {
            if !value.trim().is_empty() {
                return value;
            }
        }
    }
    "unknown".to_string()
}

pub(crate) fn now() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// A short, stable name for one incident: the stamp it was recorded at, the
/// claim, and the envelope it carries. Two incidents recorded in the same
/// millisecond with the same claim and envelope are the same incident.
pub(crate) fn identity(recorded_at: &str, claim: &str, envelope: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(recorded_at.as_bytes());
    hasher.update(b"\0");
    hasher.update(claim.as_bytes());
    hasher.update(b"\0");
    hasher.update(serde_json::to_vec(envelope).unwrap_or_default());
    hex::encode(hasher.finalize())[..IDENTITY_HEX].to_string()
}
