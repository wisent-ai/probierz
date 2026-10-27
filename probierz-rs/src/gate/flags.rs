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
