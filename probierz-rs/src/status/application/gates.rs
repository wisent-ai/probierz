//! The evidence level a run reaches. The gate config itself is `crate::gate::gate_config`.

use crate::status::*;

/// `E0` for a run that did not pass; `E3` for a passed run whose record
/// states every one of recording, report, analysis and capture true; `E2`
/// otherwise. A field the run does not state counts as not reached.
pub(crate) fn evidence_level(run: &Value) -> &'static str {
    if run.get("status").and_then(Value::as_str) != Some("passed") {
        return "E0";
    }
    let stated = |pointer: &str| run.pointer(pointer).and_then(Value::as_bool) == Some(true);
    if stated("/conditions/record")
        && stated("/evidence/report")
        && stated("/evidence/analysis")
        && stated("/evidence/capturePresent")
    {
        "E3"
    } else {
        "E2"
    }
}
