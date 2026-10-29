//! The events a run's traces and logs hold, placed in time. An event the
//! trace gives no clock or no time for stands at the trace's own recorded
//! time; a time the trace states that cannot be read is an error on the
//! event (`clock::with_instant`), never another instant put in its place.

use crate::run::*;
use serde_json::json;

// Probierz's JSON observation trace format version:
// https://github.com/wisent-ai/probierz/blob/main/guides/evidence.md
const JSON_TRACE_SCHEMA_VERSION: u64 = 1;

/// Where a Playwright trace's own clock stands: the wall time and the
/// monotonic time its context options recorded together, when both were
/// measured.
fn trace_clock(traces: &[Value]) -> Option<(f64, f64)> {
    traces.iter().find_map(|row| {
        if row.get("type").and_then(Value::as_str) != Some("context-options") {
            return None;
        }
        Some((
            measured(js_number(row.get("wallTime")))?,
            measured(js_number(row.get("monotonicTime")))?,
        ))
    })
}

pub(crate) fn trace_events(
    file: &Path,
    recorded: &str,
    diagnostics: &mut Vec<Value>,
    redaction: &manifest::Redaction,
) -> Vec<Value> {
    // Every message and address below is shown as the run's redaction allows.
    let safe_message = |text: &str| safe_message(text, redaction);
    let safe_url = |url: &str| safe_url(url, redaction);
    let parsed = (|| -> Result<Vec<Value>, String> {
        let entries = zip_entries(file)?;
        let traces: Vec<Value> = entries
            .iter()
            .filter(|(name, _)| name.ends_with(".trace"))
            .flat_map(|(_, content)| json_lines(content))
            .collect();
        let clock = trace_clock(&traces);
        let at_for = |value: f64| -> Result<String, String> {
            match (clock, measured(value)) {
                (Some((wall, monotonic)), Some(value)) => {
                    millis_iso((wall + value - monotonic) as i64)
                }
                _ => Ok(recorded.to_owned()),
            }
        };
        let source = file.file_name().and_then(|name| name.to_str());
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
            let status = measured(js_number(snapshot.pointer("/response/status"))).map(number);
            let duration = measured(js_number(snapshot.get("time"))).map(number);
            let event = json!({ "type": "network", "source": source, "method": snapshot.pointer("/request/method"), "url": safe_url(url), "status": status, "durationMs": duration });
            let at = at_for(first_nonzero(
                snapshot.get("_monotonicTime"),
                row.get("monotonicTime"),
            ));
            events.push(with_instant(event, "at", &at));
        }
        for row in &traces {
            // Older traces write a console entry's fields beside its method
            // rather than under `params`.
            let params = match row.get("params") {
                Some(params) => params,
                None => row,
            };
            let method = row
                .get("method")
                .and_then(Value::as_str)
                .map(str::to_ascii_lowercase);
            let is_console = row.get("type").and_then(Value::as_str) == Some("console")
                || matches!(
                    method.as_deref(),
                    Some("console" | "pageerror" | "page-error")
                );
            if !is_console {
                continue;
            }
            let severity = match method.as_deref() {
                Some(method) if method.contains("error") => Some("error"),
                _ => params
                    .get("type")
                    .or_else(|| params.get("messageType"))
                    .and_then(Value::as_str),
            };
            let mut event = json!({ "type": "console", "source": source, "severity": severity });
            if let Value::Object(fields) = &mut event {
                match params
                    .get("text")
                    .or_else(|| params.get("message"))
                    .and_then(Value::as_str)
                {
                    Some(message) => {
                        fields.insert("message".into(), json!(safe_message(message)));
                    }
                    None => {
                        fields.insert("message".into(), Value::Null);
                        fields.insert(
                            "messageError".into(),
                            json!("the console entry holds no text or message"),
                        );
                    }
                }
            }
            let at = at_for(first_nonzero(row.get("time"), row.get("monotonicTime")));
            events.push(with_instant(event, "at", &at));
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

pub(crate) fn json_trace_events(
    file: &Path,
    recorded: &str,
    diagnostics: &mut Vec<Value>,
    redaction: &manifest::Redaction,
) -> Vec<Value> {
    let safe_message = |text: &str| safe_message(text, redaction);
    let result = (|| -> Result<Value, String> {
        let document = crate::json_file::required(file).map_err(|error| error.detail)?;
        if document.get("schemaVersion").and_then(Value::as_u64) != Some(JSON_TRACE_SCHEMA_VERSION)
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
        Ok(document) => {
            let at = match document.get("completedAt").and_then(Value::as_str) {
                Some(stated) => normalized_iso(stated, "the trace's completedAt"),
                None => Ok(recorded.to_owned()),
            };
            let event = json!({
                "type": "observation",
                "source": file.file_name().map(|name| name.to_string_lossy().into_owned()),
                "status": document["status"],
                // The observation's reply whole, or null when it recorded none.
                "message": document.pointer("/observation/reply").and_then(Value::as_str).map(safe_message),
            });
            vec![with_instant(event, "at", &at)]
        }
        Err(error) => {
            diagnostics.push(json!({ "artifact": file, "error": error }));
            Vec::new()
        }
    }
}

/// The timestamped lines of one run log. A log the run did not write has no
/// events; one that exists and cannot be read is a diagnostic naming it, so
/// the timeline does not look like a run that printed nothing. A line whose
/// time cannot be read carries that error beside its message.
pub(crate) fn log_events(
    file: &Path,
    source: &str,
    diagnostics: &mut Vec<Value>,
    redaction: &manifest::Redaction,
) -> Vec<Value> {
    let safe_message = |text: &str| safe_message(text, redaction);
    let content = match fs::read_to_string(file) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            diagnostics.push(json!({ "artifact": file, "error": error.to_string() }));
            return Vec::new();
        }
    };
    let expression =
        Regex::new(r"^(?P<at>\d{4}-\d{2}-\d{2}T\S+)\s(?P<message>.*)$").expect("regex");
    content
        .lines()
        .filter_map(|line| {
            let groups = expression.captures(line)?;
            let event = json!({ "type": "log", "source": source, "message": safe_message(&groups["message"]) });
            Some(with_instant(event, "at", &normalized_iso(&groups["at"], "the log line's time")))
        })
        .collect()
}
