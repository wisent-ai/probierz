use serde_json::json;
use crate::run::*;
pub(crate) fn zip_entries(file: &Path) -> Result<Vec<(String, String)>, String> {
    let buffer = fs::read(file).map_err(|error| error.to_string())?;
    if buffer.len() < 22 {
        return Err("zip end record missing".into());
    }
    let minimum = buffer.len().saturating_sub(65_557);
    let mut end = None;
    for offset in (minimum..=buffer.len() - 22).rev() {
        if read_u32(&buffer, offset) == Some(0x06054b50) {
            end = Some(offset);
            break;
        }
    }
    let end = end.ok_or("zip end record missing")?;
    let count = read_u16(&buffer, end + 10).ok_or("invalid zip end record")? as usize;
    let mut offset = read_u32(&buffer, end + 16).ok_or("invalid zip end record")? as usize;
    let mut entries = Vec::new();
    for _ in 0..count {
        if read_u32(&buffer, offset) != Some(0x02014b50) {
            return Err("invalid zip central directory".into());
        }
        let method = read_u16(&buffer, offset + 10).ok_or("invalid zip central directory")?;
        let size = read_u32(&buffer, offset + 20).ok_or("invalid zip central directory")? as usize;
        let name_len =
            read_u16(&buffer, offset + 28).ok_or("invalid zip central directory")? as usize;
        let extra_len =
            read_u16(&buffer, offset + 30).ok_or("invalid zip central directory")? as usize;
        let comment_len =
            read_u16(&buffer, offset + 32).ok_or("invalid zip central directory")? as usize;
        let local = read_u32(&buffer, offset + 42).ok_or("invalid zip central directory")? as usize;
        let name = String::from_utf8_lossy(
            buffer
                .get(offset + 46..offset + 46 + name_len)
                .ok_or("invalid zip central directory")?,
        )
        .into_owned();
        if read_u32(&buffer, local) != Some(0x04034b50) {
            return Err("invalid zip local header".into());
        }
        let local_name = read_u16(&buffer, local + 26).ok_or("invalid zip local header")? as usize;
        let local_extra = read_u16(&buffer, local + 28).ok_or("invalid zip local header")? as usize;
        let data_at = local + 30 + local_name + local_extra;
        let compressed = buffer
            .get(data_at..data_at + size)
            .ok_or("invalid zip data")?;
        let content = if method == 0 {
            Some(compressed.to_vec())
        } else if method == 8 {
            let mut decoded = Vec::new();
            DeflateDecoder::new(compressed)
                .read_to_end(&mut decoded)
                .map_err(|error| error.to_string())?;
            Some(decoded)
        } else {
            None
        };
        if let Some(content) = content {
            entries.push((name, String::from_utf8_lossy(&content).into_owned()));
        }
        offset += 46 + name_len + extra_len + comment_len;
    }
    Ok(entries)
}
pub(crate) fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}
pub(crate) fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
pub(crate) fn json_lines(content: &str) -> Vec<Value> {
    content
        .lines()
        .filter_map(|line| {
            (!line.trim().is_empty())
                .then(|| serde_json::from_str(line).ok())
                .flatten()
        })
        .collect()
}

pub(crate) fn trace_events(file: &Path, fallback: &str, diagnostics: &mut Vec<Value>) -> Vec<Value> {
    let parsed = (|| -> Result<Vec<Value>, String> {
        let entries = zip_entries(file)?;
        let traces: Vec<Value> = entries
            .iter()
            .filter(|(name, _)| name.ends_with(".trace"))
            .flat_map(|(_, content)| json_lines(content))
            .collect();
        let context = traces.iter().find(|row| {
            row.get("type").and_then(Value::as_str) == Some("context-options")
                && js_number(row.get("wallTime")) != 0.0
                && js_number(row.get("monotonicTime")) != 0.0
        });
        let wall = context
            .map(|row| js_number(row.get("wallTime")))
            .unwrap_or(0.0);
        let monotonic = context
            .map(|row| js_number(row.get("monotonicTime")))
            .unwrap_or(0.0);
        let at_for = |value: f64| {
            if wall != 0.0 && value != 0.0 {
                DateTime::from_timestamp_millis((wall + value - monotonic) as i64)
                    .map(|date| date.to_rfc3339_opts(SecondsFormat::Millis, true))
                    .unwrap_or_else(|| fallback.into())
            } else {
                fallback.into()
            }
        };
        let source = file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let mut events = Vec::new();
        for row in entries
            .iter()
            .filter(|(name, _)| name.ends_with(".network"))
            .flat_map(|(_, content)| json_lines(content))
        {
            if row.get("type").and_then(Value::as_str) != Some("resource-snapshot") {
                continue;
            }
            let Some(url) = row.pointer("/snapshot/request/url").and_then(Value::as_str) else {
                continue;
            };
            let snapshot = &row["snapshot"];
            let status = js_number(snapshot.pointer("/response/status"));
            let duration = js_number(snapshot.get("time"));
            events.push(json!({ "at": at_for(first_nonzero(snapshot.get("_monotonicTime"), row.get("monotonicTime"))), "type": "network", "source": source, "method": snapshot.pointer("/request/method").cloned().unwrap_or(Value::Null), "url": safe_url(url), "status": if status == 0.0 { Value::Null } else { number(status) }, "durationMs": if duration == 0.0 { Value::Null } else { number(duration) } }));
        }
        for row in &traces {
            let params = row.get("params").unwrap_or(row);
            let method = row
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            let is_console = row.get("type").and_then(Value::as_str) == Some("console")
                || matches!(method.as_str(), "console" | "pageerror" | "page-error");
            if !is_console {
                continue;
            }
            let message = params
                .get("text")
                .or_else(|| params.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let severity = if method.contains("error") {
                "error"
            } else {
                params
                    .get("type")
                    .or_else(|| params.get("messageType"))
                    .and_then(Value::as_str)
                    .unwrap_or("log")
            };
            events.push(json!({ "at": at_for(first_nonzero(row.get("time"), row.get("monotonicTime"))), "type": "console", "source": source, "severity": severity, "message": safe_message(message).chars().take(2000).collect::<String>() }));
        }
        Ok(events)
    })();
    match parsed {
        Ok(events) => events,
        Err(error) => {
            diagnostics.push(json!({ "artifact": file, "error": error }));
            Vec::new()
        }
    }
}

pub(crate) fn json_trace_events(file: &Path, fallback: &str, diagnostics: &mut Vec<Value>) -> Vec<Value> {
    let result = (|| -> Result<Value, String> {
        let document: Value =
            serde_json::from_slice(&fs::read(file).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        if document.get("schemaVersion").and_then(Value::as_u64) != Some(1)
            || !document
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.starts_with("probierz-"))
            || document.get("status").and_then(Value::as_str) != Some("completed")
        {
            return Err("invalid Probierz JSON trace".into());
        }
        Ok(document)
    })();
    match result {
        Ok(document) => vec![
            json!({ "at": parse_iso(document.get("completedAt").and_then(Value::as_str), fallback), "type": "observation", "source": file.file_name().and_then(|name| name.to_str()).unwrap_or(""), "status": document["status"], "message": safe_message(document.pointer("/observation/reply").and_then(Value::as_str).unwrap_or("")).chars().take(2000).collect::<String>() }),
        ],
        Err(error) => {
            diagnostics.push(json!({ "artifact": file, "error": error }));
            Vec::new()
        }
    }
}

pub(crate) fn log_events(file: &Path, source: &str) -> Vec<Value> {
    let Ok(content) = fs::read_to_string(file) else {
        return Vec::new();
    };
    let fallback = modified_iso(file);
    let expression = Regex::new(r"^(\d{4}-\d{2}-\d{2}T\S+)\s(.*)$").expect("regex");
    content.lines().filter_map(|line| { let groups = expression.captures(line)?; Some(json!({ "at": parse_iso(Some(&groups[1]), &fallback), "type": "log", "source": source, "message": safe_message(&groups[2]) })) }).collect()
}

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
    events.extend(log_events(&artifacts.join("stdout.log"), "stdout"));
    events.extend(log_events(&artifacts.join("stderr.log"), "stderr"));
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
    let mut crashes: Vec<Value> = events.iter().filter(|event| event.get("type").and_then(Value::as_str) == Some("log") && error_line(event.get("message").and_then(Value::as_str).unwrap_or(""))).map(|event| json!({ "at": event["at"], "source": event["source"], "message": event.get("message").and_then(Value::as_str).unwrap_or("").chars().take(500).collect::<String>() })).collect();
    let directory = artifacts.join("diagnostics");
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("log") {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path) {
                for line in content.lines().filter(|line| error_line(line)) {
                    crashes.push(json!({ "source": path.file_name().and_then(|value| value.to_str()).unwrap_or(""), "message": safe_message(line).chars().take(500).collect::<String>() }));
                }
            }
        }
    }
    let network_errors: Vec<Value> = network.iter().filter(|event| js_number(event.get("status")) >= 400.0).map(|event| json!({ "at": event["at"], "method": event["method"], "url": event["url"], "status": event["status"] })).collect();
    let console_errors: Vec<Value> = events.iter().filter(|event| event.get("type").and_then(Value::as_str) == Some("console") && matches!(event.get("severity").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref(), Some("assert" | "error"))).map(|event| json!({ "at": event["at"], "severity": event["severity"], "message": safe_message(event.get("message").and_then(Value::as_str).unwrap_or("")).chars().take(500).collect::<String>() })).collect();
    let durations: Vec<f64> = network
        .iter()
        .map(|event| js_number(event.get("durationMs")))
        .filter(|value| *value > 0.0)
        .collect();
    let process = fs::read(artifacts.join("performance.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let process_summary = process.as_ref().map(|value| json!({ "firstOutputMs": value["firstOutputMs"], "peakRssKb": value["peakRssKb"], "averageCpuPercent": value["averageCpuPercent"], "appProcessName": value["appProcessName"], "appPeakRssKb": value["appPeakRssKb"], "appAverageCpuPercent": value["appAverageCpuPercent"] })).unwrap_or(Value::Null);
    let result = json!({ "schemaVersion": 1, "runId": report.pointer("/probierz/runId").cloned().unwrap_or(Value::Null), "crashes": crashes, "networkErrors": network_errors, "consoleErrors": console_errors, "performance": { "tests": { "count": assertions.len(), "p50Ms": percentile(assertions.clone(), 0.5), "p95Ms": percentile(assertions.clone(), 0.95), "maxMs": assertions.iter().copied().max_by(f64::total_cmp).map(number).unwrap_or(Value::Null) }, "network": { "count": durations.len(), "p50Ms": percentile(durations.clone(), 0.5), "p95Ms": percentile(durations.clone(), 0.95), "maxMs": durations.iter().copied().max_by(f64::total_cmp).map(number).unwrap_or(Value::Null) }, "process": process_summary } });
    let file = artifacts.join("diagnostics.json");
    let _ = write_json(&file, &result);
    let mut returned = Map::new();
    returned.insert("file".into(), json!(file));
    returned.extend(result.as_object().expect("object").clone());
    Value::Object(returned)
}

