use serde_json::json;
use crate::evidence::*;
pub fn audit(
    harness: &Path,
    app_id: Option<&str>,
    run_id: Option<&str>,
    action: Option<&str>,
    limit: &str,
) -> Answer {
    let parsed = limit
        .parse::<f64>()
        .map_err(|_| Failure::invalid("evidence.audit", "--limit needs a positive number"))?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return Err(Failure::invalid(
            "evidence.audit",
            "--limit needs a positive number",
        ));
    }
    let mut records = Vec::new();
    for file in audit_files(&harness.join("test-results").join(".audit"))? {
        match json_file(&file) {
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
    records.truncate((parsed as usize).max(1));
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

pub(crate) fn acceptable_secret(value: &str) -> bool {
    let environment_reference = ["env.", "source.", "process.env."].iter().any(|prefix| {
        let Some(name) = value.strip_prefix(prefix) else {
            return false;
        };
        let mut bytes = name.bytes();
        bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    });
    let uppercase_reference = value.len() >= 2
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_uppercase())
        && value
            .bytes()
            .skip(1)
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
    let placeholder = value
        .strip_prefix('<')
        .and_then(|text| text.strip_suffix('>'))
        .is_some_and(|inside| !inside.is_empty() && !inside.contains('>'));
    value.is_empty()
        || value == "[REDACTED]"
        || value.starts_with("vault:")
        || value.starts_with("${")
        || environment_reference
        || uppercase_reference
        || placeholder
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
    let rules = [
        ("private-key", Regex::new(r"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----").map_err(|e| Failure::config("evidence.secret_scan", e.to_string()))?, 0usize),
        ("aws-access-key", Regex::new(r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b").map_err(|e| Failure::config("evidence.secret_scan", e.to_string()))?, 0),
        ("github-token", Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{30,}\b").map_err(|e| Failure::config("evidence.secret_scan", e.to_string()))?, 0),
        ("slack-token", Regex::new(r"\bxox[baprs]-[A-Za-z0-9-]{20,}\b").map_err(|e| Failure::config("evidence.secret_scan", e.to_string()))?, 0),
        ("jwt", Regex::new(r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b").map_err(|e| Failure::config("evidence.secret_scan", e.to_string()))?, 0),
        ("assigned-secret", Regex::new(r#"(?i)(?:token|secret|password|api[_-]?key|authorization|cookie)["']?\s*[:=]\s*["']?([A-Za-z0-9+/_=.:-]{8,})"#).map_err(|e| Failure::config("evidence.secret_scan", e.to_string()))?, 1),
    ];
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
            for (rule, regex, capture) in &rules {
                for matched in regex.captures_iter(&line) {
                    let Some(full) = matched.get(0) else { continue };
                    let value = matched
                        .get(*capture)
                        .map(|part| part.as_str())
                        .unwrap_or_default();
                    if acceptable_secret(value) {
                        continue;
                    }
                    let column = line[..full.start()].encode_utf16().count() + 1;
                    findings.push(json!({
                        "rule": rule,
                        "file": relative,
                        "line": line_number,
                        "column": column,
                        "fingerprintSha256": sha256_bytes(value.as_bytes()),
                    }));
                    if findings.len() >= 1000 {
                        break;
                    }
                }
                if findings.len() >= 1000 {
                    break;
                }
            }
            if findings.len() >= 1000 {
                break;
            }
        }
        if findings.len() >= 1000 {
            break;
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

pub fn secret_scan(root: Option<&Path>) -> Answer {
    let root = root
        .ok_or_else(|| Failure::invalid("evidence.secret_scan", "secret-scan needs a directory"))?;
    let result = scan_secrets(root)?;
    print_json(&result)?;
    if result.get("passed").and_then(Value::as_bool) != Some(true) {
        std::process::exit(1);
    }
    Ok(())
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

