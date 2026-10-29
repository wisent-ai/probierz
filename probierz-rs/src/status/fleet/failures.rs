//! The desktop failure store: where it lives, how a service names its file, and the failures answer.

use crate::status::*;

/// Where desktop failures are stored: PROBIERZ_FAILURES_DIR, else under the
/// one Probierz home (crate::run::home_dir), never `.` without HOME, which
/// made the store a different, empty directory for every working directory.
pub(crate) fn failures_dir() -> PathBuf {
    match std::env::var_os("PROBIERZ_FAILURES_DIR").filter(|value| !value.is_empty()) {
        Some(directory) => PathBuf::from(directory),
        None => crate::run::home_dir().join(".probierz").join("failures"),
    }
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
    // Envelopes grouped by service and code: the map keeps them ordered by
    // service then code, and the stable sort puts the most frequent first.
    let mut grouped: std::collections::BTreeMap<(&str, &str), Vec<&Value>> =
        std::collections::BTreeMap::new();
    for envelope in &envelopes {
        // An envelope without its service or code is grouped under that
        // statement; "unknown" read like a service and a code of their own.
        let service = match string(envelope.get("service")) {
            Some(service) => service,
            None => "service not reported",
        };
        let code = match string(envelope.get("error_code")) {
            Some(code) => code,
            None => "code not reported",
        };
        grouped.entry((service, code)).or_default().push(envelope);
    }
    let mut grouped = grouped.into_iter().collect::<Vec<_>>();
    grouped.sort_by_key(|group| std::cmp::Reverse(group.1.len()));
    let counts = grouped
        .into_iter()
        .map(|((service, code), members)| {
            json!({
                "service": service,
                "error_code": code,
                "count": members.len(),
            })
        })
        .collect::<Vec<_>>();
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

pub(crate) fn render_failures(report: &Value) -> String {
    let mut lines = vec![
        format!(
            "failures: {} stored ({} unparsed lines) in {}",
            said(report.get("total"), "total"),
            said(report.get("unparsed"), "unparsed count"),
            said(report.get("directory"), "directory"),
        ),
        "by service and error_code:".to_string(),
    ];
    match report.get("counts").and_then(Value::as_array) {
        None => lines.push("  (no counts)".to_string()),
        Some(counts) if counts.is_empty() => lines.push("  (none)".to_string()),
        Some(counts) => {
            for row in counts {
                lines.push(format!(
                    "  {}  {}  {}",
                    said(row.get("service"), "service"),
                    said(row.get("error_code"), "error_code"),
                    said(row.get("count"), "count"),
                ));
            }
        }
    }
    lines.push("newest:".to_string());
    let newest = match report.get("newest").and_then(Value::as_array) {
        None => {
            lines.push("  (no newest list)".to_string());
            Vec::new()
        }
        Some(newest) if newest.is_empty() => {
            lines.push("  (none)".to_string());
            Vec::new()
        }
        Some(newest) => newest.clone(),
    };
    for envelope in newest {
        // An envelope without a detail adds nothing after its fields.
        let detail = match string(envelope.get("detail")) {
            Some(detail) => format!("  {}", detail.trim()),
            None => String::new(),
        };
        // Each field the envelope carries, or the field named as missing: a
        // dash or "undefined" read as a time, service or code of that name.
        let field = |name: &str| match string(envelope.get(name)) {
            Some(value) => value.to_string(),
            None => format!("(no {name})"),
        };
        lines.push(format!(
            "  {}  {}  {}  {}{}",
            field("received_at"),
            field("service"),
            field("error_code"),
            field("failure_point"),
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
