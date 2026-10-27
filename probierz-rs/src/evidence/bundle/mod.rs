//! A run's artifacts as an encrypted bundle: the cipher it is sealed with,
//! the protection that seals it, the restore that opens it, and the
//! retention that eventually removes it.

mod cipher;
mod fleet_retention;
mod protect;
mod restore;
mod retention;

pub(crate) use cipher::*;
pub use fleet_retention::*;
pub use protect::*;
pub use restore::*;
pub use retention::*;

use crate::failure::{Answer, Failure};
use serde_json::Value;

/// The answer once an allowed action's audit was attempted: the action
/// stands, but an audit that could not be written is the command's failure,
/// where it used to be dropped and the trail silently missed the access.
pub(crate) fn allowed_audit_written(action: &str, written: Result<Value, Failure>) -> Answer {
    written.map(|_audit_record| ()).map_err(|audit| {
        Failure::new(
            "evidence.audit",
            audit.code,
            format!("{action} was carried out, but its access audit could not be written: {audit}"),
        )
    })
}

/// A refusal, carrying the reason its denied-access audit could not be
/// written when that also failed.
pub(crate) fn refusal_audited(error: Failure, written: Result<Value, Failure>) -> Failure {
    match written {
        Ok(_) => error,
        Err(audit) => Failure {
            detail: format!(
                "{}; its access audit could not be written: {audit}",
                error.detail
            ),
            ..error
        },
    }
}
