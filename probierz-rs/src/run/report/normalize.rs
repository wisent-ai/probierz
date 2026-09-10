use serde_json::json;
use crate::run::*;
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
        let duration = js_number(row.get("duration"));
        let mut test = Map::new();
        test.insert(
            "title".into(),
            row.get("title").cloned().unwrap_or(Value::Null),
        );
        test.insert("passed".into(), Value::Bool(status == "passed"));
        test.insert("status".into(), Value::String(status.clone()));
        test.insert("durationMs".into(), number(duration));
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
    let duration: f64 = tests
        .iter()
        .map(|test| js_number(test.get("durationMs")))
        .sum();
    json!({ "tool": tool, "total": report.get("total").cloned().unwrap_or_else(|| json!(tests.len())), "passed": count("passed"), "failed": count("failed"), "flaky": number(js_number(report.get("flaky"))), "skipped": count("skipped"), "durationMs": number(duration), "tests": tests, "failures": failures, "reportMedia": media })
}
pub(crate) fn first_nonzero(primary: Option<&Value>, fallback: Option<&Value>) -> f64 {
    let primary = js_number(primary);
    if primary != 0.0 {
        primary
    } else {
        js_number(fallback)
    }
}

pub(crate) fn visit_playwright(
    suite: &Value,
    tests: &mut Vec<Value>,
    failures: &mut Vec<Value>,
    media: &mut Vec<Value>,
) {
    for spec in suite
        .get("specs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mut statuses = Vec::new();
        let mut duration = 0.0;
        let mut error: Option<String> = None;
        for test in spec
            .get("tests")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for result in test
                .get("results")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                statuses.push(
                    result
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                );
                duration += js_number(result.get("duration"));
                for attachment in result
                    .get("attachments")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if attachment.get("path").and_then(Value::as_str).is_some() {
                        let mut item = Map::new();
                        item.insert("file".into(), attachment["path"].clone());
                        item.insert(
                            "kind".into(),
                            attachment.get("name").cloned().unwrap_or(Value::Null),
                        );
                        if let Some(content_type) = attachment.get("contentType") {
                            item.insert("contentType".into(), content_type.clone());
                        }
                        media.push(Value::Object(item));
                    }
                }
                if result.get("status").and_then(Value::as_str) != Some("passed") {
                    if let Some(first) = result
                        .get("errors")
                        .and_then(Value::as_array)
                        .and_then(|errors| errors.first())
                    {
                        error = Some(
                            first
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .lines()
                                .next()
                                .unwrap_or("")
                                .to_string(),
                        );
                    }
                }
            }
        }
        let status = if spec.get("ok").and_then(Value::as_bool) == Some(false) {
            "failed"
        } else if !statuses.is_empty() && statuses.iter().all(|status| status == "skipped") {
            "skipped"
        } else {
            "passed"
        };
        tests.push(json!({ "title": spec.get("title").cloned().unwrap_or(Value::Null), "passed": status == "passed", "status": status, "durationMs": number(duration) }));
        if status == "failed" {
            failures.push(json!({ "title": spec.get("title").cloned().unwrap_or(Value::Null), "error": error.filter(|value| !value.is_empty()).unwrap_or_else(|| "failed".into()) }));
        }
    }
    for child in suite
        .get("suites")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        visit_playwright(child, tests, failures, media);
    }
}

pub(crate) fn normalize_playwright(report: &Value) -> Value {
    let mut tests = Vec::new();
    let mut failures = Vec::new();
    let mut media = Vec::new();
    for suite in report
        .get("suites")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        visit_playwright(suite, &mut tests, &mut failures, &mut media);
    }
    let count = |wanted: &str| {
        tests
            .iter()
            .filter(|test| test.get("status").and_then(Value::as_str) == Some(wanted))
            .count()
    };
    json!({ "tool": "playwright", "total": tests.len(), "passed": count("passed"), "failed": count("failed"), "flaky": number(js_number(report.pointer("/stats/flaky"))), "skipped": count("skipped"), "durationMs": number(js_number(report.pointer("/stats/duration"))), "tests": tests, "failures": failures, "reportMedia": media })
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

pub(crate) fn safe_message(value: &str) -> String {
    let mut result = Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]+")
        .expect("regex")
        .replace_all(value, "Bearer [REDACTED]")
        .into_owned();
    result = Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b")
        .expect("regex")
        .replace_all(&result, "[EMAIL]")
        .into_owned();
    Regex::new(r"(?i)((?:auth|cookie|credential|key|otp|password|secret|session|token)[A-Z0-9_.-]*\s*[=:]\s*)[^\s,;]+").expect("regex").replace_all(&result, "$1[REDACTED]").into_owned()
}

pub(crate) fn safe_url(value: &str) -> String {
    if let Ok(mut url) = Url::parse(value) {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_fragment(None);
        let sensitive = Regex::new(
            r"(?i)(auth|code|cookie|credential|email|key|otp|password|secret|session|token)",
        )
        .expect("regex");
        let source: Vec<(String, String)> = url
            .query_pairs()
            .map(|(name, _)| {
                let replacement = if sensitive.is_match(&name) {
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
                || (sensitive.is_match(&previous) && !decoded.is_empty())
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

