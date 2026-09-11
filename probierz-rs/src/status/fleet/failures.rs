//! The desktop failure store: where it lives, how a service names its file, and the failures answer.

use crate::status::*;

pub(crate) fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub(crate) fn failures_dir() -> PathBuf {
    std::env::var_os("PROBIERZ_FAILURES_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".probierz").join("failures"))
}

pub(crate) fn service_file_name(service: &str) -> String {
    let mut replaced = String::new();
    let mut invalid_run = false;
    for byte in service.trim().to_ascii_lowercase().bytes() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' {
            invalid_run = false;
            replaced.push(byte as char);
        } else if !invalid_run {
            replaced.push('-');
            invalid_run = true;
        }
    }
    let clean = replaced.trim_matches('-');
    format!("{}.jsonl", if clean.is_empty() { "unknown" } else { clean })
}

pub(crate) fn log_failure_index(detail: &str) {
    eprintln!(
        "probierz-failure {}",
        json!({
            "failure_point": "cli.failures",
            "error_code": "unknown",
            "service": "cli",
            "impact": "cli",
            "severity": "error",
            "retryable": false,
            "outage": false,
            "detail": detail,
        })
    );
}

pub(crate) fn failures_index(service: Option<&str>, limit: usize) -> Value {
    let directory = failures_dir();
    let mut names = Vec::new();
    if directory.exists() {
        match fs::read_dir(&directory) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let name = entry.file_name().to_string_lossy().into_owned();
                            if name.ends_with(".jsonl") {
                                names.push(name);
                            }
                        }
                        Err(error) => {
                            log_failure_index(&format!("list the failures index: {error}"))
                        }
                    }
                }
                names.sort();
            }
            Err(error) => log_failure_index(&format!("list the failures index: {error}")),
        }
    }
    if let Some(service) = service {
        let wanted = service_file_name(service);
        names.retain(|name| name == &wanted);
    }
    let mut envelopes = Vec::new();
    let mut unparsed = 0usize;
    for name in &names {
        match fs::read_to_string(directory.join(name)) {
            Ok(text) => {
                for line in text.lines().filter(|line| !line.trim().is_empty()) {
                    match serde_json::from_str::<Value>(line) {
                        Ok(envelope) => envelopes.push(envelope),
                        Err(_) => unparsed += 1,
                    }
                }
            }
            Err(error) => log_failure_index(&format!("read {name}: {error}")),
        }
    }
    // JavaScript joins the two grouping fields with an embedded NUL, then
    // splits on that same byte. Keep it explicit here so source tooling does
    // not silently hide the separator.
    let mut grouped: Vec<(String, usize)> = Vec::new();
    for envelope in &envelopes {
        let service = string(envelope.get("service")).unwrap_or("unknown");
        let code = string(envelope.get("error_code")).unwrap_or("unknown");
        let key = format!("{service}\0{code}");
        if let Some((_, count)) = grouped.iter_mut().find(|entry| entry.0 == key) {
            *count += 1;
        } else {
            grouped.push((key, 1));
        }
    }
    let mut counts = grouped
        .into_iter()
        .map(|(key, count)| {
            let (service, code) = key.split_once('\0').unwrap_or((&key, ""));
            json!({
                "service": service,
                "error_code": code,
                "count": count,
            })
        })
        .collect::<Vec<_>>();
    counts.sort_by(|left, right| {
        number(right.get("count"))
            .partial_cmp(&number(left.get("count")))
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                string(left.get("service"))
                    .unwrap_or("")
                    .cmp(string(right.get("service")).unwrap_or(""))
            })
            .then_with(|| {
                string(left.get("error_code"))
                    .unwrap_or("")
                    .cmp(string(right.get("error_code")).unwrap_or(""))
            })
    });
    let mut ordered = envelopes.iter().enumerate().collect::<Vec<_>>();
    ordered.sort_by(|(left_index, left), (right_index, right)| {
        string(left.get("received_at"))
            .unwrap_or("")
            .cmp(string(right.get("received_at")).unwrap_or(""))
            .then_with(|| left_index.cmp(right_index))
    });
    let take = if limit == 0 { ordered.len() } else { limit };
    let newest = ordered
        .into_iter()
        .rev()
        .take(take)
        .map(|(_, envelope)| envelope.clone())
        .collect::<Vec<_>>();
    json!({
        "directory": directory.to_string_lossy(),
        "services": names.iter().map(|name| Value::String(name.trim_end_matches(".jsonl").to_string())).collect::<Vec<_>>(),
        "total": envelopes.len(),
        "unparsed": unparsed,
        "counts": counts,
        "newest": newest,
    })
}

pub(crate) fn trim_detail(text: &str, limit: usize) -> String {
    let value = text.trim();
    value.chars().take(limit).collect()
}

pub(crate) fn render_failures(report: &Value) -> String {
    let mut lines = vec![
        format!(
            "failures: {} stored ({} unparsed lines) in {}",
            report.get("total").and_then(Value::as_u64).unwrap_or(0),
            report.get("unparsed").and_then(Value::as_u64).unwrap_or(0),
            report
                .get("directory")
                .and_then(Value::as_str)
                .unwrap_or(""),
        ),
        "by service and error_code:".to_string(),
    ];
    let counts = report
        .get("counts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if counts.is_empty() {
        lines.push("  (none)".to_string());
    }
    for row in counts {
        lines.push(format!(
            "  {}  {}  {}",
            row.get("service").and_then(Value::as_str).unwrap_or(""),
            row.get("error_code").and_then(Value::as_str).unwrap_or(""),
            row.get("count").and_then(Value::as_u64).unwrap_or(0),
        ));
    }
    lines.push("newest:".to_string());
    let newest = report
        .get("newest")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if newest.is_empty() {
        lines.push("  (none)".to_string());
    }
    for envelope in newest {
        let detail = string(envelope.get("detail"))
            .map(|detail| format!("  {}", trim_detail(detail, 160)))
            .unwrap_or_default();
        lines.push(format!(
            "  {}  {}  {}  {}{}",
            string(envelope.get("received_at")).unwrap_or("-"),
            envelope
                .get("service")
                .and_then(Value::as_str)
                .unwrap_or("undefined"),
            envelope
                .get("error_code")
                .and_then(Value::as_str)
                .unwrap_or("undefined"),
            envelope
                .get("failure_point")
                .and_then(Value::as_str)
                .unwrap_or("undefined"),
            detail,
        ));
    }
    lines.join("\n")
}

pub fn failures(service: Option<&str>, limit: usize, output_json: bool) -> Answer {
    let report = failures_index(service, limit);
    if output_json {
        print_json(&report)
    } else {
        println!("{}", render_failures(&report));
        Ok(())
    }
}
