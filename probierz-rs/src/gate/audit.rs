use crate::gate::*;

/// The access record a gate evaluation leaves behind, written by Probierz's
/// one audit writer (`crate::evidence::audit_access`), which hides the
/// details the product's manifest declares. A gate decides about no single
/// run, so the record names none.
pub(crate) fn audit_access(
    harness: &Path,
    action: &str,
    outcome: &str,
    app_id: Option<&str>,
    resource: Option<&str>,
    details: Value,
) -> Result<(), Failure> {
    crate::evidence::audit_access(
        harness,
        action,
        outcome,
        app_id,
        None,
        resource.map(Path::new),
        details,
    )
    .map(|_| ())
}
