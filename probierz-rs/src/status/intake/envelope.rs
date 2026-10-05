//! The failure intake envelope: its failure point, codes and meaning, the JSON line it becomes, and the store it is appended to.

use crate::status::*;

pub(crate) const ERROR_CODES: [&str; 7] = [
    "config",
    "auth",
    "not_found",
    "rate_limit",
    "timeout",
    "infra_down",
    "unknown",
];

pub(crate) fn valid_failure_point(point: &str) -> bool {
    if point.is_empty() {
        return false;
    }
    point.split('.').all(|segment| {
        let bytes = segment.as_bytes();
        if bytes.is_empty() || !bytes[0].is_ascii_lowercase() {
            return false;
        }
        let mut previous_separator = false;
        for byte in &bytes[1..] {
            if *byte == b'-' || *byte == b'_' {
                if previous_separator {
                    return false;
                }
                previous_separator = true;
            } else if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
                previous_separator = false;
            } else {
                return false;
            }
        }
        !previous_separator
    })
}

pub(crate) fn envelope_problem(body: &Value) -> Option<String> {
    if !body.is_object() {
        return Some("body is not a JSON object".to_string());
    }
    if !body
        .get("failure_point")
        .and_then(Value::as_str)
        .is_some_and(valid_failure_point)
    {
        return Some("failure_point must be a dotted lowercase path".to_string());
    }
    let code = body.get("error_code").and_then(Value::as_str);
    if !code.is_some_and(|code| ERROR_CODES.contains(&code)) {
        return Some(format!(
            "error_code must be one of {}",
            ERROR_CODES.join(", ")
        ));
    }
    if !body
        .get("service")
        .and_then(Value::as_str)
        .is_some_and(|service| !service.trim().is_empty())
    {
        return Some("service must be a non-empty string".to_string());
    }
    None
}

pub(crate) fn code_meaning(code: &str) -> (&'static str, bool, bool) {
    match code {
        "config" => ("critical", false, true),
        "auth" | "not_found" => ("warning", false, false),
        "rate_limit" => ("warning", true, false),
        "timeout" => ("error", true, true),
        "infra_down" => ("critical", true, true),
        _ => ("error", false, false),
    }
}

pub(crate) fn failure_envelope(code: &str, detail: &str) -> Value {
    let (severity, retryable, outage) = code_meaning(code);
    json!({
        "failure_point": "probierz.intake.request",
        "error_code": code,
        "service": "probierz-intake",
        "impact": "failure-intake",
        "severity": severity,
        "retryable": retryable,
        "outage": outage,
        "detail": detail.trim(),
    })
}

pub(crate) fn authorized(header: Option<&str>, token: &str) -> bool {
    let presented = header
        .and_then(|header| header.strip_prefix("Bearer "))
        .unwrap_or("");
    if presented.len() != token.len() {
        return false;
    }
    presented
        .bytes()
        .zip(token.bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

/// The envelope as one stored JSON line, stamped with when it arrived. Nothing is dropped
/// to fit a size: the context, cause and detail a service reported are kept whole.
pub(crate) fn stored_line(envelope: &Value) -> Result<String, Failure> {
    let mut stored = envelope.clone();
    stored
        .as_object_mut()
        .ok_or_else(|| Failure::invalid("intake.envelope", "body is not a JSON object"))?
        .insert("received_at".to_string(), Value::String(now()));
    Ok(serde_json::to_string(&stored)?)
}

/// Append the envelope to its service's failure file. Every failure is kept: the file is not
/// rotated or halved at a size chosen here.
pub(crate) fn store_envelope(envelope: &Value) -> Result<(), Failure> {
    let directory = failures_dir();
    fs::create_dir_all(&directory)?;
    let line = stored_line(envelope)?;
    let service = envelope
        .get("service")
        .and_then(Value::as_str)
        .unwrap_or("");
    let file = directory.join(service_file_name(service));
    let mut output = OpenOptions::new().create(true).append(true).open(file)?;
    output.write_all(line.as_bytes())?;
    output.write_all(b"\n")?;
    Ok(())
}
