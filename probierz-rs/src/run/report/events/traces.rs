use serde_json::json;
use crate::run::*;
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

