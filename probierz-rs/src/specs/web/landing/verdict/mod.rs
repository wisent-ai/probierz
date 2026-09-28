//! The verdict and the report a person reads afterwards.
//!
//! It states what the page was judged against, what was measured at every
//! viewport, what the model said about each graded dimension, and the verdict
//! that follows — so a failure can be understood without re-running anything.
//! A deterministic finding caps the model's score for the dimension it
//! contradicts, and blocks the release by itself.

use serde_json::{json, Map};

use super::constants::{
    CAP_CONVERSION, CAP_HEADING, CAP_HIDDEN_ACTION, CAP_MISSING_ALT, CAP_NOT_SERVED, CAP_OVERFLOW,
    CAP_STRUCTURE, CAP_UNNAMED_CONTROLS, CAP_UNSTABLE, HTTP_SERVED, LAYOUT_SHIFT_LIMIT,
    OVERFLOW_TOLERANCE_PX, ROUTER_ATTEMPTS, SCHEMA_VERSION, SCORE_PRECISION,
};
use crate::specs::*;

pub(super) mod judge;

struct Findings {
    dimensions: Map<String, Value>,
    blockers: Vec<Value>,
}

impl Findings {
    fn cap(&mut self, name: &str, maximum: f64, issue: String) {
        let Some(dimension) = self.dimensions.get_mut(name) else {
            return;
        };
        let adjusted = dimension["adjustedScore"]
            .as_f64()
            .unwrap_or_default()
            .min(maximum);
        dimension["adjustedScore"] = json!(adjusted);
        if let Some(issues) = dimension["issues"].as_array_mut() {
            if !issues.contains(&Value::String(issue.clone())) {
                issues.push(Value::String(issue));
            }
        }
    }

    fn block(&mut self, code: &str, evidence: String, source: &str) {
        self.blockers
            .push(json!({ "code": code, "evidence": evidence, "source": source }));
    }
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap_or_default()
}

fn count(value: &Value) -> usize {
    value.as_array().map(Vec::len).unwrap_or_default()
}

fn audit_findings(findings: &mut Findings, audit: &Value, label: &str) {
    let profile = audit["profile"].as_str().unwrap_or_default();
    let status = audit["httpStatus"].as_u64().unwrap_or_default();
    if !HTTP_SERVED.contains(&status) {
        findings.block(
            "http_status",
            format!("{profile}: HTTP {status}"),
            "browser",
        );
        findings.cap(
            "performance_stability",
            CAP_NOT_SERVED,
            format!("{profile} did not receive a successful document response"),
        );
    }
    let headings = count(&audit["h1"]);
    if headings != 1 {
        findings.block(
            "page_heading",
            format!("{profile}: {headings} visible h1 elements"),
            "browser",
        );
        findings.cap(
            "message_clarity",
            CAP_HEADING,
            format!("{profile} must expose exactly one visible h1"),
        );
        findings.cap(
            "accessibility_semantics",
            CAP_HEADING,
            format!("{profile} heading contract is invalid"),
        );
    }
    let shown = audit["primaryActionMatches"]
        .as_array()
        .is_some_and(|matches| matches.iter().any(|entry| entry["inFirstViewport"] == true));
    if !shown {
        findings.block(
            "primary_action",
            format!(
                "{profile}: {} is not visible in the first viewport",
                json!(label)
            ),
            "browser",
        );
        findings.cap(
            "hierarchy_conversion",
            CAP_HIDDEN_ACTION,
            format!("{profile} hides the primary action below the first viewport"),
        );
        findings.cap(
            "conversion_continuity",
            CAP_HIDDEN_ACTION,
            format!("{profile} does not expose the approved action immediately"),
        );
    }
    let overflow = number(&audit["horizontalOverflowPx"]);
    if overflow > OVERFLOW_TOLERANCE_PX {
        findings.block(
            "horizontal_overflow",
            format!("{profile}: {overflow}px"),
            "browser",
        );
        findings.cap(
            "responsive_behavior",
            CAP_OVERFLOW,
            format!("{profile} overflows horizontally by {overflow}px"),
        );
    }
    let unnamed = number(&audit["unnamedInteractiveCount"]);
    let unlabeled = number(&audit["unlabeledFormControlCount"]);
    if unnamed > 0.0 || unlabeled > 0.0 {
        let evidence = format!("{profile}: {unnamed} unnamed interactive controls, {unlabeled} unlabeled form controls");
        findings.block("accessible_controls", evidence, "browser");
        findings.cap(
            "accessibility_semantics",
            CAP_UNNAMED_CONTROLS,
            format!("{profile} contains controls without accessible names or labels"),
        );
    }
    let missing_alt = number(&audit["imagesMissingAltCount"]);
    if missing_alt > 0.0 {
        findings.cap(
            "accessibility_semantics",
            CAP_MISSING_ALT,
            format!("{profile} has {missing_alt} informative images without alt attributes"),
        );
    }
    let (skips, duplicates) = (
        number(&audit["headingLevelSkips"]),
        number(&audit["duplicateIdCount"]),
    );
    if skips > 0.0 || duplicates > 0.0 {
        findings.cap(
            "accessibility_semantics",
            CAP_STRUCTURE,
            format!("{profile} has {skips} heading-level skips and {duplicates} duplicate ids"),
        );
    }
    let shift = number(&audit["cumulativeLayoutShift"]);
    let (failed, errors) = (
        count(&audit["failedRequests"]),
        count(&audit["consoleErrors"]),
    );
    if shift > LAYOUT_SHIFT_LIMIT || failed > 0 || errors > 0 {
        let issue =
            format!("{profile}: CLS {shift:.3}, {failed} failed requests, {errors} console errors");
        findings.cap("performance_stability", CAP_UNSTABLE, issue);
    }
}

/// The report of one evaluation; `pass` is true only with no blocker.
pub(super) fn report(
    rubric: &Value,
    brief_path: &Path,
    brief: &Value,
    audits: &Value,
    conversion: (bool, String),
    model: &Value,
    images: &[(String, PathBuf)],
) -> Value {
    let evaluation = &model["evaluation"];
    let mut dimensions = evaluation["dimensions"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    for dimension in dimensions.values_mut() {
        dimension["adjustedScore"] = dimension["score"].clone();
    }
    let mut findings = Findings {
        dimensions,
        blockers: Vec::new(),
    };
    let label = brief["primaryAction"]["label"].as_str().unwrap_or_default();
    for audit in audits.as_object().into_iter().flat_map(Map::values) {
        audit_findings(&mut findings, audit, label);
    }
    let (converts, evidence) = conversion;
    if !converts {
        findings.block("conversion_target", evidence.clone(), "browser");
        findings.cap("conversion_continuity", CAP_CONVERSION, evidence.clone());
    }
    for issue in evaluation["blocking_issues"]
        .as_array()
        .into_iter()
        .flatten()
    {
        findings.blockers.push(
            json!({ "code": issue["code"], "evidence": issue["evidence"], "source": "model" }),
        );
    }
    let rules = rubric["dimensions"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let adjusted = |name: &str| number(&findings.dimensions[name]["adjustedScore"]);
    let overall = rules
        .iter()
        .map(|(name, rule)| adjusted(name) * number(&rule["weight"]))
        .sum::<f64>();
    let overall = (overall * SCORE_PRECISION).round() / SCORE_PRECISION;
    let mut thresholds = Vec::new();
    for (name, rule) in &rules {
        let (score, minimum) = (adjusted(name), number(&rule["minimum"]));
        if score < minimum {
            let evidence = format!("{score:.3} < {minimum:.3}");
            thresholds.push(json!({ "code": format!("dimension_below_minimum:{name}"), "evidence": evidence, "source": "threshold" }));
        }
    }
    let required = number(&rubric["overallMinimum"]);
    if overall < required {
        let evidence = format!("{overall:.3} < {required:.3}");
        thresholds.push(
            json!({ "code": "overall_below_minimum", "evidence": evidence, "source": "threshold" }),
        );
    }
    findings.blockers.extend(thresholds);
    let captures: Vec<Value> = images
        .iter()
        .map(|(label, path)| json!({ "label": label, "path": path }))
        .collect();
    json!({
        "schemaVersion": SCHEMA_VERSION,
        "rubric": { "name": rubric["name"], "overallMinimum": rubric["overallMinimum"], "dimensions": rubric["dimensions"] },
        "brief": {
            "path": brief_path,
            "product": brief["product"],
            "audience": brief["audience"],
            "promise": brief["promise"],
            "primaryAction": brief["primaryAction"],
            "analyticsOwner": brief["analyticsOwner"],
        },
        "pass": findings.blockers.is_empty(),
        "overall": overall,
        "summary": evaluation["summary"],
        "dimensions": findings.dimensions,
        "blockers": findings.blockers,
        "recommendations": evaluation["recommendations"],
        "conversion": { "pass": converts, "evidence": evidence },
        "audits": audits,
        "captures": captures,
        "router": { "model": model["routerModel"], "usage": model["usage"], "attempts": ROUTER_ATTEMPTS },
    })
}
