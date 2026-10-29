use crate::run::*;
use serde_json::json;
pub(crate) fn normalize_wdio(report: &Value, tool: &str) -> Value {
    let rows = report
        .get("tests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut tests = Vec::new();
    let mut failures = Vec::new();
    let mut media = Vec::new();
    for row in &rows {
        let status = row
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                if row.get("passed").and_then(Value::as_bool).unwrap_or(false) {
                    "passed".into()
                } else {
                    "failed".into()
                }
            });
        let duration = row.get("duration").and_then(Value::as_f64);
        let mut test = Map::new();
        test.insert(
            "title".into(),
            row.get("title").cloned().unwrap_or(Value::Null),
        );
        test.insert("passed".into(), Value::Bool(status == "passed"));
        test.insert("status".into(), Value::String(status.clone()));
        test.insert("durationMs".into(), json!(duration.map(number)));
        if let Some(video) = row.get("video") {
            test.insert("video".into(), video.clone());
        }
        tests.push(Value::Object(test));
        if status == "failed" {
            let error = row
                .get("error")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .unwrap_or("failed");
            failures.push(json!({ "title": row.get("title").cloned().unwrap_or(Value::Null), "error": error }));
        }
        if let Some(items) = row.get("media").and_then(Value::as_array) {
            media.extend(items.clone());
        }
        if let Some(video) = row.get("video").filter(|value| !value.is_null()) {
            media.push(json!({ "file": video, "kind": "video" }));
        }
    }
    let count = |wanted: &str| {
        tests
            .iter()
            .filter(|test| test.get("status").and_then(Value::as_str) == Some(wanted))
            .count()
    };
    // A run's duration is the sum of its tests' only when every test states
    // one; a report that states no flaky count has none, not zero.
    let duration: Option<f64> = tests
        .iter()
        .map(|test| test.get("durationMs").and_then(Value::as_f64))
        .sum();
    let total = match report.get("total") {
        Some(total) => total.clone(),
        None => json!(tests.len()),
    };
    let flaky = report.get("flaky").and_then(Value::as_f64).map(number);
    json!({ "tool": tool, "total": total, "passed": count("passed"), "failed": count("failed"), "flaky": flaky, "skipped": count("skipped"), "durationMs": duration.map(number), "tests": tests, "failures": failures, "reportMedia": media })
}
pub(crate) fn first_nonzero(primary: Option<&Value>, fallback: Option<&Value>) -> f64 {
    let primary = js_number(primary);
    if primary != 0.0 {
        primary
    } else {
        js_number(fallback)
    }
}

pub(crate) fn parse_iso(value: Option<&str>, fallback: &str) -> String {
    value
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|date| {
            date.with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        })
        .unwrap_or_else(|| fallback.to_string())
}

/// `value` as a report may show it: bearer tokens and addresses replaced by
/// their shape, and every value the run's redaction covers replaced.
pub(crate) fn safe_message(value: &str, redaction: &manifest::Redaction) -> String {
    let mut result = Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]+")
        .expect("regex")
        .replace_all(value, "Bearer [REDACTED]")
        .into_owned();
    result = Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b")
        .expect("regex")
        .replace_all(&result, "[EMAIL]")
        .into_owned();
    redaction.text(&result)
}

/// `value` as a report may show it: credentials and fragments dropped, every
/// query value replaced, and a path segment hidden when it is an address, a
/// token-shaped word, or follows a segment the run's redaction covers.
pub(crate) fn safe_url(value: &str, redaction: &manifest::Redaction) -> String {
    if let Ok(mut url) = Url::parse(value) {
        // A URL without a host (mailto:, data:, file:) refuses credentials,
        // so it holds none to clear; that refusal is the only error here.
        match url.set_username("").and_then(|()| url.set_password(None)) {
            Ok(()) | Err(()) => {}
        }
        url.set_fragment(None);
        let source: Vec<(String, String)> = url
            .query_pairs()
            .map(|(name, _)| {
                let replacement = if redaction.covers(&name) {
                    "[REDACTED]"
                } else {
                    "[VALUE]"
                };
                (name.into_owned(), replacement.into())
            })
            .collect();
        if !source.is_empty() {
            url.query_pairs_mut().clear().extend_pairs(source);
        }
        let segments: Vec<String> = url.path().split('/').map(str::to_string).collect();
        let mut safe = Vec::new();
        for (index, segment) in segments.iter().enumerate() {
            let decoded = percent_decode(segment);
            let previous = index
                .checked_sub(1)
                .map(|at| percent_decode(&segments[at]))
                .unwrap_or_default();
            if decoded.contains('@')
                || (decoded.len() >= 32
                    && decoded
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'))
                || (redaction.covers(&previous) && !decoded.is_empty())
            {
                safe.push("%5BREDACTED%5D".into());
            } else {
                safe.push(segment.clone());
            }
        }
        url.set_path(&safe.join("/"));
        return url
            .to_string()
            .replace("%255B", "%5B")
            .replace("%255D", "%5D");
    }
    Regex::new(r"([?&][^=]+)=([^&\s]+)")
        .expect("regex")
        .replace_all(value, "$1=[VALUE]")
        .into_owned()
}

pub(crate) fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
