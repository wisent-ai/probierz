//! The gate's read of the yes-or-no fields evidence carries: a verdict's and a
//! receipt's `passed` and `valid`, a run's `record`, `report`, `analysis` and
//! `capturePresent`, its protection's `plaintextRemoved` and its secret scan's
//! `passed`. Probierz writes every one of them as a JSON boolean
//! (run/drive/execute/record/manifest.rs and the receipt and verdict writers),
//! so a boolean is the only shape that can say yes.

use crate::gate::*;

/// Whether the field holds JSON `true`. An absent field, null, `false` and
/// any value of another type (the string "false", a number, a list) are not
/// a yes, so a malformed verdict blocks the gate instead of passing it.
pub(crate) fn is_true(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true)))
}

/// The evidence level a run reaches, the one rule the gate, status, receipts,
/// protected bundles and matrix cells read: `E0` for a run that did not pass;
/// `E3` for a passed run whose conditions state `record` and whose evidence
/// states `report`, `analysis` and `capturePresent` true; `E2` otherwise. A
/// field the run does not state counts as not reached.
pub(crate) fn level_of(passed: bool, conditions: Option<&Value>, evidence: Option<&Value>) -> &'static str {
    if !passed {
        return "E0";
    }
    let stated = |object: Option<&Value>, name: &str| is_true(object.and_then(|object| property(object, name)));
    if stated(conditions, "record")
        && stated(evidence, "report")
        && stated(evidence, "analysis")
        && stated(evidence, "capturePresent")
    {
        "E3"
    } else {
        "E2"
    }
}

/// [`level_of`] for a run manifest read as JSON: it passed when its status
/// is `passed`.
pub(crate) fn run_evidence_level(run: &Value) -> &'static str {
    level_of(
        run.get("status").and_then(Value::as_str) == Some("passed"),
        run.get("conditions"),
        run.get("evidence"),
    )
}
