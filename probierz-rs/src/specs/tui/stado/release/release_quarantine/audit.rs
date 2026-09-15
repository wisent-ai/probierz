//! The one owner-only audit line a clear leaves: who, why, which entry it destroyed,
//! the quarantine instant it kept, and the backup it names.

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::specs::tui::stado::fleet_fixture::{self as fixture, FIXTURE_HOST, FIXTURE_PRODUCT};

use super::QUARANTINE_REASON;

pub(super) fn check_audit(
    audit_path: &Path,
    digest: &str,
    reason: &str,
    quarantined_at: &str,
    backup: &str,
) -> Result<Value, String> {
    let audit_text = fs::read_to_string(audit_path).map_err(|error| error.to_string())?;
    let audit_lines = audit_text.trim().lines().collect::<Vec<_>>();
    fixture::ensure(
        audit_lines.len() == 1,
        format!("audit has {} lines instead of one", audit_lines.len()),
    )?;
    let audit: Value = serde_json::from_str(audit_lines[0]).map_err(|error| error.to_string())?;
    fixture::ensure(
        audit["host"] == FIXTURE_HOST
            && audit["product"] == FIXTURE_PRODUCT
            && audit["digest"] == digest
            && audit["reason"] == reason
            && audit["quarantine_reason"] == QUARANTINE_REASON,
        format!("audit line is wrong: {audit}"),
    )?;
    let original =
        chrono::DateTime::parse_from_rfc3339(quarantined_at).map_err(|error| error.to_string())?;
    let audited_original =
        chrono::DateTime::parse_from_rfc3339(audit["quarantined_at"].as_str().unwrap_or_default())
            .map_err(|error| error.to_string())?;
    fixture::ensure(
        original == audited_original,
        "audit changed the quarantine instant",
    )?;
    fixture::ensure(
        audit["state_backup"] == backup,
        format!("audit names the wrong backup: {audit}"),
    )?;
    fixture::ensure(audit["actor"].is_string(), "the audit line names no actor")?;
    fixture::ensure(
        audit["audited_at"].is_string(),
        "the audit line carries no instant",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fixture::ensure(
            fs::metadata(audit_path)
                .map_err(|error| error.to_string())?
                .permissions()
                .mode()
                & 0o777
                == 0o600,
            "the audit trail is not owner-only",
        )?;
    }
    Ok(audit)
}
