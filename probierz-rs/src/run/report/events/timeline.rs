use crate::run::*;
use serde_json::json;
pub(crate) fn modified_iso(path: &Path) -> String {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .map(system_time_iso)
        .unwrap_or_else(now_iso)
}
pub(crate) fn system_time_iso(time: SystemTime) -> String {
    let millis = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    DateTime::from_timestamp_millis(millis)
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn build_timeline(
    report: &Value,
    summary: &Value,
    media: &[Value],
    artifacts: &Path,
    started: Option<&str>,
) -> Value {
    let mut diagnostics = Vec::new();
    let fallback = parse_iso(started, &now_iso());
    let mut events = Vec::new();
    events.extend(log_events(
        &artifacts.join("stdout.log"),
        "stdout",
        &mut diagnostics,
    ));
    events.extend(log_events(
        &artifacts.join("stderr.log"),
        "stderr",
        &mut diagnostics,
    ));
    let mut cursor = DateTime::parse_from_rfc3339(&fallback)
        .map(|date| date.timestamp_millis())
        .unwrap_or_else(|_| Utc::now().timestamp_millis());
    for test in report
        .get("tests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let duration = js_number(test.get("duration").or_else(|| test.get("durationMs")));
        let default_at = DateTime::from_timestamp_millis(cursor)
            .unwrap_or_else(Utc::now)
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let at = parse_iso(test.get("startedAt").and_then(Value::as_str), &default_at);
        let start_ms = DateTime::parse_from_rfc3339(&at)
            .map(|date| date.timestamp_millis())
            .unwrap_or(cursor);
        let default_completed = DateTime::from_timestamp_millis(start_ms + duration as i64)
            .unwrap_or_else(Utc::now)
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let completed = parse_iso(
            test.get("completedAt").and_then(Value::as_str),
            &default_completed,
        );
        events.push(json!({ "at": at, "completedAt": completed, "durationMs": duration, "type": "assertion", "source": summary.get("tool").cloned().unwrap_or(Value::Null), "title": test.get("title").cloned().unwrap_or(Value::Null), "status": test.get("status").cloned().unwrap_or_else(|| Value::String(if test.get("passed").and_then(Value::as_bool).unwrap_or(false) { "passed" } else { "failed" }.into())), "error": test.get("error").cloned().unwrap_or(Value::Null) }));
        cursor = DateTime::parse_from_rfc3339(&completed)
            .map(|date| date.timestamp_millis())
            .unwrap_or(cursor);
    }
    for item in media {
        let file = PathBuf::from(item.get("file").and_then(Value::as_str).unwrap_or(""));
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
        let at = if file.exists() {
            modified_iso(&file)
        } else {
            fallback.clone()
        };
        events.push(json!({ "at": at, "type": if kind == "screenshot" { "screenshot" } else { kind }, "source": summary.get("tool").cloned().unwrap_or(Value::Null), "artifact": file, "missing": item.get("missing").and_then(Value::as_bool).unwrap_or(false) }));
        if kind == "trace" && file.exists() {
            if item.get("contentType").and_then(Value::as_str) == Some("application/json") {
                events.extend(json_trace_events(&file, &at, &mut diagnostics));
            } else {
                events.extend(trace_events(&file, &at, &mut diagnostics));
            }
        }
    }
    events.sort_by(|left, right| {
        left.get("at")
            .and_then(Value::as_str)
            .cmp(&right.get("at").and_then(Value::as_str))
            .then(
                left.get("type")
                    .and_then(Value::as_str)
                    .cmp(&right.get("type").and_then(Value::as_str)),
            )
    });
    let mut counts = Map::new();
    let types: BTreeSet<String> = events
        .iter()
        .filter_map(|event| {
            event
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    for kind in types {
        counts.insert(
            kind.clone(),
            json!(events
                .iter()
                .filter(|event| event.get("type").and_then(Value::as_str) == Some(&kind))
                .count()),
        );
    }
    json!({ "schemaVersion": 1, "runId": report.pointer("/probierz/runId").cloned().unwrap_or(Value::Null), "artifactsDir": artifacts, "generatedAt": now_iso(), "counts": counts, "diagnostics": diagnostics, "events": events })
}

pub(crate) fn percentile(mut values: Vec<f64>, fraction: f64) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    let index = ((values.len() as f64 * fraction).ceil() as usize)
        .saturating_sub(1)
        .min(values.len() - 1);
    number(values[index])
}
pub(crate) fn error_line(value: &str) -> bool {
    Regex::new(r"(?i)\b(?:crash(?:ed)?|fatal|panic|uncaught|unhandled|segmentation fault|assertion failed)\b").expect("regex").is_match(value)
}

pub(crate) fn summarize_diagnostics(report: &Value, timeline: &Value, artifacts: &Path) -> Value {
    let assertions: Vec<f64> = report
        .get("tests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|test| js_number(test.get("duration").or_else(|| test.get("durationMs"))))
        .filter(|value| value.is_finite())
        .collect();
    let events = timeline
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let network: Vec<&Value> = events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("network"))
        .collect();
    let mut crashes = Vec::new();
    for event in events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("log"))
    {
        let Some(message) = event.get("message").and_then(Value::as_str) else {
            crashes.push(json!({
                "at": event["at"],
                "source": event["source"],
                "error": "timeline log event has no string message; crash classification is unavailable"
            }));
            continue;
        };
        if error_line(message) {
            crashes.push(json!({
                "at": event["at"],
                "source": event["source"],
                "message": safe_message(message)
            }));
        }
    }
    // A diagnostics log that could not be listed or read is a crash source
    // whose content is unknown: it is entered with its error, where it used to
    // be skipped and the crash list read as complete.
    let directory = artifacts.join("diagnostics");
    match fs::read_dir(&directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => crashes.push(json!({
            "source": directory,
            "error": format!("diagnostics directory cannot be listed: {error}")
        })),
        Ok(entries) => {
            for entry in entries {
                let path = match entry {
                    Ok(entry) => entry.path(),
                    Err(error) => {
                        crashes.push(json!({
                            "source": directory,
                            "error": format!("a diagnostics entry cannot be read: {error}")
                        }));
                        continue;
                    }
                };
                if !path.is_file()
                    || path.extension().and_then(|value| value.to_str()) != Some("log")
                {
                    continue;
                }
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        for line in content.lines().filter(|line| error_line(line)) {
                            crashes.push(json!({ "source": path, "message": safe_message(line) }));
                        }
                    }
                    Err(error) => crashes.push(json!({
                        "source": path,
                        "error": format!("diagnostics log cannot be read: {error}")
                    })),
                }
            }
        }
    }
    let network_errors: Vec<Value> = network.iter().filter(|event| js_number(event.get("status")) >= 400.0).map(|event| json!({ "at": event["at"], "method": event["method"], "url": event["url"], "status": event["status"] })).collect();
    let mut console_errors = Vec::new();
    for event in events.iter().filter(|event| {
        event.get("type").and_then(Value::as_str) == Some("console")
            && matches!(
                event
                    .get("severity")
                    .and_then(Value::as_str)
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("assert" | "error")
            )
    }) {
        match event.get("message").and_then(Value::as_str) {
            Some(message) => console_errors.push(json!({
                "at": event["at"],
                "severity": event["severity"],
                "message": safe_message(message)
            })),
            None => console_errors.push(json!({
                "at": event["at"],
                "severity": event["severity"],
                "error": "timeline console error has no string message; its diagnostic text is unavailable"
            })),
        }
    }
    let durations: Vec<f64> = network
        .iter()
        .map(|event| js_number(event.get("durationMs")))
        .filter(|value| *value > 0.0)
        .collect();
    let process_summary = performance_summary(&artifacts.join("performance.json"));
    let result = json!({ "schemaVersion": 1, "runId": report.pointer("/probierz/runId").cloned().unwrap_or(Value::Null), "crashes": crashes, "networkErrors": network_errors, "consoleErrors": console_errors, "performance": { "tests": { "count": assertions.len(), "p50Ms": percentile(assertions.clone(), 0.5), "p95Ms": percentile(assertions.clone(), 0.95), "maxMs": assertions.iter().copied().max_by(f64::total_cmp).map(number).unwrap_or(Value::Null) }, "network": { "count": durations.len(), "p50Ms": percentile(durations.clone(), 0.5), "p95Ms": percentile(durations.clone(), 0.95), "maxMs": durations.iter().copied().max_by(f64::total_cmp).map(number).unwrap_or(Value::Null) }, "process": process_summary } });
    let file = artifacts.join("diagnostics.json");
    let mut returned = Map::new();
    returned.insert("file".into(), json!(file));
    if let Err(error) = write_json(&file, &result) {
        returned.insert("fileError".into(), json!(format!("{error}")));
    }
    returned.extend(result.as_object().expect("object").clone());
    Value::Object(returned)
}

/// The run's process performance, `null` when the run recorded none, or the
/// reason `performance.json` could not be read or parsed - which used to read
/// as "no performance recorded".
fn performance_summary(path: &Path) -> Value {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Value::Null,
        Err(error) => {
            return json!({ "error": format!("{} cannot be read: {error}", path.display()) })
        }
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => json!({
            "firstOutputMs": value["firstOutputMs"],
            "peakRssKb": value["peakRssKb"],
            "averageCpuPercent": value["averageCpuPercent"],
            "appProcessName": value["appProcessName"],
            "appPeakRssKb": value["appPeakRssKb"],
            "appAverageCpuPercent": value["appAverageCpuPercent"]
        }),
        Err(error) => json!({ "error": format!("{} is not JSON: {error}", path.display()) }),
    }
}
