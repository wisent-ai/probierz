use crate::evidence::*;
use serde_json::json;
pub fn audit(
    harness: &Path,
    app_id: Option<&str>,
    run_id: Option<&str>,
    action: Option<&str>,
    limit: Option<&str>,
) -> Answer {
    let parsed = match limit {
        Some(limit) => {
            let parsed = limit.parse::<f64>().map_err(|_| {
                Failure::invalid("evidence.audit", "--limit needs a positive number")
            })?;
            if !parsed.is_finite() || parsed <= 0.0 {
                return Err(Failure::invalid(
                    "evidence.audit",
                    "--limit needs a positive number",
                ));
            }
            Some((parsed as usize).max(1))
        }
        None => None,
    };
    let mut records = Vec::new();
    for file in audit_files(&harness.join("test-results").join(".audit"))? {
        match crate::json_file::required(&file) {
            Ok(record) => {
                if app_id.is_some_and(|wanted| {
                    record.get("appId").and_then(Value::as_str) != Some(wanted)
                }) || run_id.is_some_and(|wanted| {
                    record.get("runId").and_then(Value::as_str) != Some(wanted)
                }) || action.is_some_and(|wanted| {
                    record.get("action").and_then(Value::as_str) != Some(wanted)
                }) {
                    continue;
                }
                let mut payload = record.as_object().cloned().unwrap_or_default();
                let expected = payload
                    .remove("sha256")
                    .and_then(|value| value.as_str().map(str::to_string));
                let valid = expected.as_deref()
                    == Some(&sha256_bytes(
                        serde_json::to_string(&stable(&Value::Object(payload)))?.as_bytes(),
                    ));
                let mut output = record.as_object().cloned().unwrap_or_default();
                output.insert("valid".into(), json!(valid));
                output.insert("file".into(), json!(file.to_string_lossy()));
                records.push(Value::Object(output));
            }
            Err(error) => records.push(
                json!({ "valid": false, "file": file.to_string_lossy(), "error": error.detail }),
            ),
        }
    }
    records.sort_by(|left, right| {
        right
            .get("at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(left.get("at").and_then(Value::as_str).unwrap_or_default())
    });
    let total = records.len();
    if let Some(parsed) = parsed {
        records.truncate(parsed);
    }
    let valid = records
        .iter()
        .filter(|record| record.get("valid").and_then(Value::as_bool) == Some(true))
        .count();
    print_json(&json!({
        "schemaVersion": 1,
        "filters": { "appId": app_id, "runId": run_id, "action": action },
        "total": total,
        "returned": records.len(),
        "valid": valid,
        "invalid": records.len() - valid,
        "records": records,
    }))
}

pub(crate) fn is_binary(file: &Path) -> Result<bool, Failure> {
    let mut input = File::open(file)?;
    let mut sample = [0u8; 8192];
    let count = input.read(&mut sample)?;
    Ok(sample[..count].contains(&0))
}

pub fn scan_secrets(root: &Path) -> Result<Value, Failure> {
    let display_root = root.to_string_lossy().into_owned();
    let root = absolute(root)?;
    if !root.is_dir() {
        return Err(Failure::invalid(
            "evidence.secret_scan",
            format!("secret scan root is not a directory: {display_root}"),
        ));
    }
    // The formats a credential is issued in, each read from its own grammar:
    // a PEM private-key armor line, an AWS access key id, a GitHub or Slack
    // token prefix, and a three-part JWT. Which names hold secrets is the
    // manifest's redaction (`manifest::Redaction`), applied wherever a run
    // writes; it is not guessed here from words a line contains.
    let rules = [
        ("private-key", r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
        ("aws-access-key", r"\bA[KS]IA[A-Z0-9]{16}\b"),
        ("github-token", r"\bgh[pousr]_[A-Za-z0-9]{30,}\b"),
        ("slack-token", r"\bxox[baprs]-[A-Za-z0-9-]{20,}\b"),
        (
            "jwt",
            r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b",
        ),
    ]
    .into_iter()
    .map(|(rule, pattern)| {
        Regex::new(pattern)
            .map(|regex| (rule, regex))
            .map_err(|error| Failure::config("evidence.secret_scan", error.to_string()))
    })
    .collect::<Result<Vec<_>, _>>()?;
    let mut findings = Vec::new();
    let mut scanned_files = 0usize;
    let mut skipped_binary = 0usize;
    let mut skipped_generated = 0usize;
    for file in files_below(&root, false)? {
        let relative = file
            .strip_prefix(&root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        if relative.starts_with("html-report/trace/assets/") {
            skipped_generated += 1;
            continue;
        }
        if is_binary(&file)? {
            skipped_binary += 1;
            continue;
        }
        scanned_files += 1;
        let mut input = BufReader::new(File::open(&file)?);
        let mut line_bytes = Vec::new();
        let mut line_number = 0usize;
        loop {
            line_bytes.clear();
            if input.read_until(b'\n', &mut line_bytes)? == 0 {
                break;
            }
            if line_bytes.last() == Some(&b'\n') {
                line_bytes.pop();
            }
            if line_bytes.last() == Some(&b'\r') {
                line_bytes.pop();
            }
            line_number += 1;
            let line = String::from_utf8_lossy(&line_bytes);
            for (rule, regex) in &rules {
                for matched in regex.find_iter(&line) {
                    // Every format above starts with an ASCII character, so
                    // the UTF-16 units up to and including the match's first
                    // byte are its one-based column.
                    let column = line[..=matched.start()].encode_utf16().count();
                    findings.push(json!({
                        "rule": rule,
                        "file": relative,
                        "line": line_number,
                        "column": column,
                        "fingerprintSha256": sha256_bytes(matched.as_str().as_bytes()),
                    }));
                }
            }
        }
    }
    Ok(json!({
        "schemaVersion": 1,
        "kind": "probierz-secret-scan",
        "root": root.to_string_lossy(),
        "scannedAt": now_iso(),
        "scannedFiles": scanned_files,
        "skippedBinary": skipped_binary,
        "skippedGenerated": skipped_generated,
        "passed": findings.is_empty(),
        "findings": findings,
    }))
}

pub(crate) fn assert_no_secrets(root: &Path) -> Result<Value, Failure> {
    let result = scan_secrets(root)?;
    let report = root.join("diagnostics").join("secret-scan.json");
    if let Some(parent) = report.parent() {
        fs::create_dir_all(parent)?;
    }
    write_json(&report, &result)?;
    if result.get("passed").and_then(Value::as_bool) != Some(true) {
        let count = result
            .get("findings")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        return Err(Failure::invalid(
            "evidence.secret_scan",
            format!("secret scan failed with {count} finding(s)"),
        ));
    }
    Ok(result)
}
