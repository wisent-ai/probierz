//! The evidence level a run reaches. The gate config itself is `crate::gate::gate_config`.

use crate::status::*;

pub(crate) fn evidence_level(run: &Value) -> &'static str {
    if run.get("status").and_then(Value::as_str) != Some("passed") {
        return "E0";
    }
    let recorded = run
        .pointer("/conditions/record")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let report = run
        .pointer("/evidence/report")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let analysis = run
        .pointer("/evidence/analysis")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let capture = run
        .pointer("/evidence/capturePresent")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if recorded && report && analysis && capture {
        "E3"
    } else {
        "E2"
    }
}
