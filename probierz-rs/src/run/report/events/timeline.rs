//! The run's timeline, its logs, assertions, media and traces placed in
//! time, and the diagnostics summary read from it. An instant or a status
//! the run's files do not state is never invented: the event carries null
//! beside the error that says why (`clock::with_instant`).

use crate::run::*;
use serde_json::json;

// The timeline's and the diagnostics summary's schema version:
// https://github.com/wisent-ai/probierz/blob/main/guides/evidence.md
const SCHEMA_VERSION: u64 = 1;
// The median the diagnostics summary reports as p50:
// https://github.com/wisent-ai/probierz/blob/main/guides/evidence.md
const MEDIAN: f64 = 0.5;
// The 95th percentile the diagnostics summary reports as p95:
// https://github.com/wisent-ai/probierz/blob/main/guides/evidence.md
const NINETY_FIFTH: f64 = 0.95;
// The first HTTP status that reports a failed request, client errors (4xx)
// and server errors (5xx) after it: https://www.rfc-editor.org/rfc/rfc9110#section-15
const FIRST_ERROR_STATUS: f64 = 400.0;

/// One assertion on the timeline, and when it ended. A test that states no
/// start begins when the one before it ended (the run start for the first),
/// and one that states no end ends its duration after its start.
fn assertion_event(
    test: &Value,
    tool: &Option<Value>,
    previous_end: &Result<i64, String>,
) -> (Value, Result<i64, String>) {
    let duration = js_number(test.get("duration").or_else(|| test.get("durationMs")));
    let start = match test.get("startedAt").and_then(Value::as_str) {
        Some(stated) => instant_ms(stated, "the test's startedAt"),
        None => previous_end.clone().map_err(|cause| {
            format!("the test states no startedAt, and when the one before it ended is unknown: {cause}")
        }),
    };
    let end = match test.get("completedAt").and_then(Value::as_str) {
        Some(stated) => instant_ms(stated, "the test's completedAt"),
        None => start.clone().and_then(|began| {
            began.checked_add(duration as i64).ok_or_else(|| {
                format!("the test's start, {began} ms, and its duration, {duration} ms, exceed a millisecond count")
            })
        }),
    };
    let status = match (test.get("status"), test.get("passed").and_then(Value::as_bool)) {
        (Some(status), _) => Ok(status.clone()),
        (None, Some(true)) => Ok(json!("passed")),
        (None, Some(false)) => Ok(json!("failed")),
        (None, None) => Err("the test states neither its status nor whether it passed"),
    };
    let mut event = json!({ "durationMs": duration, "type": "assertion", "source": tool, "title": test.get("title"), "error": test.get("error") });
    if let Value::Object(fields) = &mut event {
        match status {
            Ok(status) => {
                fields.insert("status".into(), status);
            }
            Err(cause) => {
                fields.insert("status".into(), Value::Null);
                fields.insert("statusError".into(), json!(cause));
            }
        }
    }
    let event = with_instant(event, "at", &start.and_then(millis_iso));
    let event = with_instant(event, "completedAt", &end.clone().and_then(millis_iso));
    (event, end)
}

/// One media item on the timeline, with the events its trace holds. A media
/// file the run did not leave has no time of its own: it stands at the run
/// start, marked missing.
fn media_events(
    item: &Value,
    tool: &Option<Value>,
    run_start: &str,
    diagnostics: &mut Vec<Value>,
    redaction: &manifest::Redaction,
) -> Vec<Value> {
    let Some(file) = item.get("file").and_then(Value::as_str) else {
        diagnostics.push(json!({ "media": item, "error": "the media item names no file, so it has no place on the timeline" }));
        return Vec::new();
    };
    let Some(kind) = item.get("kind").and_then(Value::as_str) else {
        diagnostics.push(json!({ "artifact": file, "error": "the media item states no kind" }));
        return Vec::new();
    };
    let file = PathBuf::from(file);
    let (at, present) = match fs::metadata(&file) {
        Ok(_) => (modified_iso(&file), true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Ok(run_start.to_owned()), false),
        Err(error) => (Err(format!("{} cannot be examined: {error}", file.display())), false),
    };
    let missing = matches!(item.get("missing"), Some(Value::Bool(true)));
    let mut events = vec![with_instant(
        json!({ "type": kind, "source": tool, "artifact": file, "missing": missing }),
        "at",
        &at,
    )];
    if kind == "trace" && present {
        match &at {
            Ok(recorded) if item.get("contentType").and_then(Value::as_str) == Some("application/json") => {
                events.extend(json_trace_events(&file, recorded, diagnostics, redaction));
            }
            Ok(recorded) => events.extend(trace_events(&file, recorded, diagnostics, redaction)),
            Err(cause) => diagnostics.push(json!({ "artifact": file, "error": format!("the trace's events are not placed: {cause}") })),
        }
    }
    events
}

pub(crate) fn build_timeline(
    report: &Value,
    summary: &Value,
    media: &[Value],
    artifacts: &Path,
    started: Option<&str>,
    redaction: &manifest::Redaction,
) -> Value {
    let mut diagnostics = Vec::new();
    let run_start = parse_iso(started, &now_iso());
    let mut events = Vec::new();
    events.extend(log_events(&artifacts.join("stdout.log"), "stdout", &mut diagnostics, redaction));
    events.extend(log_events(&artifacts.join("stderr.log"), "stderr", &mut diagnostics, redaction));
    let tool = summary.get("tool").cloned();
    let mut previous_end = instant_ms(&run_start, "the run start");
    for test in report.get("tests").and_then(Value::as_array).into_iter().flatten() {
        let (event, end) = assertion_event(test, &tool, &previous_end);
        events.push(event);
        previous_end = end;
    }
    for item in media {
        events.extend(media_events(item, &tool, &run_start, &mut diagnostics, redaction));
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
        .filter_map(|event| event.get("type").and_then(Value::as_str).map(str::to_string))
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
    json!({ "schemaVersion": SCHEMA_VERSION, "runId": report.pointer("/probierz/runId"), "artifactsDir": artifacts, "generatedAt": now_iso(), "counts": counts, "diagnostics": diagnostics, "events": events })
}

/// The `fraction` percentile of `values` as a JSON number, null for none:
/// the one percentile is `status::percentile`, encoded here as `number`.
pub(crate) fn percentile(values: Vec<f64>, fraction: f64) -> Value {
    json!(crate::status::percentile(values, fraction).map(number))
}
pub(crate) fn error_line(value: &str) -> bool {
    Regex::new(r"(?i)\b(?:crash(?:ed)?|fatal|panic|uncaught|unhandled|segmentation fault|assertion failed)\b").expect("regex").is_match(value)
}

/// The crash lines of the run's diagnostics logs. A log that could not be
/// listed or read is a crash source whose content is unknown: it is entered
/// with its error, so the crash list is never read as complete without it.
fn diagnostics_log_crashes(artifacts: &Path, redaction: &manifest::Redaction, crashes: &mut Vec<Value>) {
    let directory = artifacts.join("diagnostics");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            crashes.push(json!({ "source": directory, "error": format!("diagnostics directory cannot be listed: {error}") }));
            return;
        }
    };
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(error) => {
                crashes.push(json!({ "source": directory, "error": format!("a diagnostics entry cannot be read: {error}") }));
                continue;
            }
        };
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("log") {
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(content) => {
                for line in content.lines().filter(|line| error_line(line)) {
                    crashes.push(json!({ "source": path, "message": safe_message(line, redaction) }));
                }
            }
            Err(error) => crashes.push(json!({ "source": path, "error": format!("diagnostics log cannot be read: {error}") })),
        }
    }
}

pub(crate) fn summarize_diagnostics(
    report: &Value,
    timeline: &Value,
    artifacts: &Path,
    redaction: &manifest::Redaction,
) -> Value {
    let assertions: Vec<f64> = report
        .get("tests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|test| js_number(test.get("duration").or_else(|| test.get("durationMs"))))
        .filter(|value| value.is_finite())
        .collect();
    let mut crashes = Vec::new();
    let events: &[Value] = match timeline.get("events").and_then(Value::as_array) {
        Some(events) => events,
        None => {
            crashes.push(json!({ "source": "timeline", "error": "the timeline holds no events array, so its logs, network and console errors are unknown" }));
            &[]
        }
    };
    let network: Vec<&Value> = events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("network"))
        .collect();
    for event in events.iter().filter(|event| event.get("type").and_then(Value::as_str) == Some("log")) {
        let Some(message) = event.get("message").and_then(Value::as_str) else {
            crashes.push(json!({ "at": event["at"], "source": event["source"], "error": "timeline log event has no string message; crash classification is unavailable" }));
            continue;
        };
        if error_line(message) {
            crashes.push(json!({ "at": event["at"], "source": event["source"], "message": safe_message(message, redaction) }));
        }
    }
    diagnostics_log_crashes(artifacts, redaction, &mut crashes);
    let network_errors: Vec<Value> = network.iter().filter(|event| js_number(event.get("status")) >= FIRST_ERROR_STATUS).map(|event| json!({ "at": event["at"], "method": event["method"], "url": event["url"], "status": event["status"] })).collect();
    let mut console_errors = Vec::new();
    for event in events.iter().filter(|event| {
        event.get("type").and_then(Value::as_str) == Some("console")
            && matches!(
                event.get("severity").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref(),
                Some("assert" | "error")
            )
    }) {
        match event.get("message").and_then(Value::as_str) {
            Some(message) => console_errors.push(json!({ "at": event["at"], "severity": event["severity"], "message": safe_message(message, redaction) })),
            None => console_errors.push(json!({ "at": event["at"], "severity": event["severity"], "error": "timeline console error has no string message; its diagnostic text is unavailable" })),
        }
    }
    let durations: Vec<f64> = network
        .iter()
        .filter_map(|event| measured(js_number(event.get("durationMs"))))
        .collect();
    let process_summary = performance_summary(&artifacts.join("performance.json"));
    let result = json!({ "schemaVersion": SCHEMA_VERSION, "runId": report.pointer("/probierz/runId"), "crashes": crashes, "networkErrors": network_errors, "consoleErrors": console_errors, "performance": { "tests": { "count": assertions.len(), "p50Ms": percentile(assertions.clone(), MEDIAN), "p95Ms": percentile(assertions.clone(), NINETY_FIFTH), "maxMs": assertions.iter().copied().max_by(f64::total_cmp).map(number) }, "network": { "count": durations.len(), "p50Ms": percentile(durations.clone(), MEDIAN), "p95Ms": percentile(durations.clone(), NINETY_FIFTH), "maxMs": durations.iter().copied().max_by(f64::total_cmp).map(number) }, "process": process_summary } });
    let file = artifacts.join("diagnostics.json");
    // The file is named only once it holds the result; a failed write names
    // the path it was meant for beside its error, as platform diagnostics do.
    let mut returned = Map::new();
    match write_json(&file, &result) {
        Ok(()) => {
            returned.insert("file".into(), json!(file));
        }
        Err(error) => {
            returned.insert("file".into(), Value::Null);
            returned.insert("attemptedFile".into(), json!(file));
            returned.insert("fileError".into(), json!(format!("{error}")));
        }
    }
    if let Value::Object(fields) = result {
        returned.extend(fields);
    }
    Value::Object(returned)
}

/// The run's process performance, `null` when the run recorded none, or the
/// reason `performance.json` could not be read or parsed.
fn performance_summary(path: &Path) -> Value {
    match crate::json_file::read(path) {
        Ok(None) => Value::Null,
        Ok(Some(value)) => json!({
            "firstOutputMs": value["firstOutputMs"],
            "peakRssKb": value["peakRssKb"],
            "averageCpuPercent": value["averageCpuPercent"],
            "appProcessName": value["appProcessName"],
            "appPeakRssKb": value["appPeakRssKb"],
            "appAverageCpuPercent": value["appAverageCpuPercent"]
        }),
        Err(error) => json!({ "error": error.detail }),
    }
}
