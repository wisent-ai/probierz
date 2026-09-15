//! Brama's `subscriptions list` and the refusals around it, run against
//! a real vault listing and a real usage ledger.
//!
//! | part | what it owns |
//! |---|---|
//! | `documents` | the vault listing and the ledger the fixture writes |
//! | `fixture` | where they live, the stub router, and the environment |
//! | `report` | what the report must say, row by row |
//! | `journey` | the run, the refusals, and the trace it writes |
//!
//! Every part opens with `use super::*;`, so the list below is the
//! journey's single import list. The three helpers every part uses to
//! run the executable and read the tree live here too.

pub(crate) use crate::specs::{self, tui::common};
pub(crate) use serde_json::{json, Value};
pub(crate) use sha2::{Digest, Sha256};
pub(crate) use std::collections::{BTreeMap, BTreeSet};
pub(crate) use std::fs;
pub(crate) use std::path::Path;
pub(crate) use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod documents;
mod fixture;
mod journey;
mod report;

pub(crate) use documents::*;
pub(crate) use fixture::*;
pub(crate) use journey::*;
pub(crate) use report::*;

/// How long any one CLI invocation may take.
pub(crate) const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// A vault payload that must never reach a report. It is in every
/// fixture item, so any leak shows up as this exact string.
pub(crate) const VAULT_PAYLOAD: &str = "probierz-fixture-vault-payload-must-never-be-printed";

/// Run the executable with the fixture's environment.
pub(crate) fn invoke(
    binary: &str,
    args: &[&str],
    env: &BTreeMap<String, String>,
) -> Result<common::Output, String> {
    common::run(
        binary,
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        None,
        env,
        &[],
        None,
        COMMAND_TIMEOUT,
    )
}

/// The JSON object a command answered with.
pub(crate) fn parsed(out: &common::Output, label: &str) -> Result<Value, String> {
    let combined = out.combined();
    let start = combined
        .find('{')
        .ok_or_else(|| format!("{label} emitted no JSON"))?;
    serde_json::from_str(combined[start..].trim())
        .map_err(|e| format!("{label} emitted a non-structured JSON value: {e}"))
}

/// Every file under a directory with its size, digest and modification
/// time, so "nothing was written" can be shown rather than assumed.
pub(crate) fn fingerprint(root: &Path) -> Result<String, String> {
    fn walk(root: &Path, rows: &mut Vec<String>) -> Result<(), String> {
        if !root.exists() {
            return Ok(());
        }
        let mut entries = fs::read_dir(root)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if p.is_dir() {
                rows.push(format!("{}/", p.display()));
                walk(&p, rows)?;
            } else {
                let bytes = fs::read(&p).map_err(|e| e.to_string())?;
                let modified = e
                    .metadata()
                    .map_err(|e| e.to_string())?
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                rows.push(format!(
                    "{}\t{}\t{}\t{}",
                    p.display(),
                    bytes.len(),
                    hex::encode(Sha256::digest(&bytes)),
                    modified
                ));
            }
        }
        Ok(())
    }
    let mut rows = Vec::new();
    walk(root, &mut rows)?;
    Ok(rows.join("\n"))
}
