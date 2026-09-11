//! The fleet overview: violations per root, fleet health, and the overview answer.

use crate::status::*;

pub(crate) fn violations_for(root: &str) -> Value {
    let output = Command::new("tama")
        .args(["find-violations", "--repo", root, "--json"])
        .output();
    let Ok(output) = output else {
        return json!({ "error": "exit null" });
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && !stdout.trim().starts_with('{') {
        let detail = stderr
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                format!(
                    "exit {}",
                    output
                        .status
                        .code()
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "null".to_string())
                )
            });
        return json!({ "error": detail });
    }
    let Ok(report) = serde_json::from_str::<Value>(&stdout) else {
        return json!({ "error": "scanner output not parseable" });
    };
    let repo = report.pointer("/repos/0").unwrap_or(&report);
    json!({
        "violations": repo.get("violations").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
        "skipped": repo.get("skippedFiles").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
        "errors": repo.get("errors").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
    })
}

pub(crate) fn fleet_failure(point: &str, code: &str, detail: &str, message: &str) -> Value {
    let (severity, retryable, outage) = code_meaning(code);
    let detail = trim_detail(detail, 300);
    eprintln!(
        "probierz-failure {}",
        json!({
            "failure_point": point,
            "error_code": code,
            "service": "objects",
            "impact": "fleet-health",
            "severity": severity,
            "retryable": retryable,
            "outage": outage,
            "detail": detail,
        })
    );
    fleet_summary(point, code, message)
}

pub(crate) fn fleet_summary(point: &str, code: &str, message: &str) -> Value {
    let (_, retryable, outage) = code_meaning(code);
    json!({
        "available": false,
        "failurePoint": point,
        "errorCode": code,
        "service": "objects",
        "retryable": retryable,
        "outage": outage,
        "message": message,
    })
}

pub(crate) fn object_message(action: &str, code: &str) -> String {
    let blame = match code {
        "infra_down" => format!("{action}: the objects dependency is unavailable. This is an infrastructure outage, not your configuration — retry later."),
        "timeout" => format!("{action}: the objects dependency did not answer in time. Not your configuration — retry later."),
        "rate_limit" => format!("{action}: the objects dependency is rate-limiting us. Retry later."),
        "config" => format!("{action}: the objects dependency is missing configuration. See the detail on the line above; retrying will not help."),
        "auth" => format!("{action}: the objects dependency rejected our credentials. Refresh them; retrying will not help."),
        "not_found" => format!("{action}: the objects dependency has no such object. Check the identifier; retrying will not help."),
        _ => format!("{action}: the objects dependency failed in a way probierz does not recognise. See the detail on the line above."),
    };
    if matches!(code, "infra_down" | "timeout" | "rate_limit") {
        format!("{blame} Local runs are unaffected — `probierz run <target>` still works without the stado queue.")
    } else {
        blame
    }
}

pub(crate) fn fleet_health() -> Value {
    let objects = match crate::evidence::list_objects("stado://probierz/capacity/") {
        Ok(objects) => objects,
        Err(failure) => {
            let code = match failure.code {
                Code::Config => "config",
                Code::Unavailable => "infra_down",
                Code::Invalid => "unknown",
                Code::Prerequisite => "config",
                Code::Refused => "unknown",
                Code::Unknown => "unknown",
            };
            let action = match failure.point.as_str() {
                "objects.config" => "Stado object storage is unusable",
                "objects.read" if failure.detail.contains("rejected") => {
                    "Stado object storage rejected the request"
                }
                "objects.read" => "Stado object storage did not answer",
                _ => "objects.list failed",
            };
            let message = object_message(action, code);
            return if matches!(failure.point.as_str(), "objects.config" | "objects.read") {
                fleet_failure(&failure.point, code, &failure.detail, &message)
            } else {
                fleet_summary("objects.list", code, &message)
            };
        }
    };
    let epoch = Utc::now().timestamp_millis();
    let mut agents = objects
        .iter()
        .filter_map(|object| {
            let updated = object.get("updated_at").and_then(Value::as_str)?;
            let updated = chrono::DateTime::parse_from_rfc3339(updated)
                .ok()?
                .with_timezone(&Utc);
            let name = object
                .get("key")
                .and_then(Value::as_str)?
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())?;
            Some((
                name.to_string(),
                updated.to_rfc3339_opts(SecondsFormat::Millis, true),
                epoch - updated.timestamp_millis() <= 900_000,
            ))
        })
        .collect::<Vec<_>>();
    agents.sort_by(|left, right| left.0.cmp(&right.0));
    json!({
        "available": true,
        "live": agents.iter().filter(|agent| agent.2).map(|agent| Value::String(agent.0.clone())).collect::<Vec<_>>(),
        "stale": agents.iter().filter(|agent| !agent.2).map(|agent| Value::String(agent.0.clone())).collect::<Vec<_>>(),
    })
}

pub(crate) fn overview_value(
    harness: &Path,
    app_ids: Option<&[String]>,
    include_violations: bool,
) -> Result<Value, Failure> {
    let ids = match app_ids {
        Some(ids) => ids.to_vec(),
        None => manifest::list(harness)?
            .into_iter()
            .map(|app| app.app_id)
            .collect(),
    };
    let mut apps = Vec::new();
    for app_id in ids {
        let status = app_status_value(harness, &app_id, "origin/main")?;
        let root = status
            .pointer("/repositories/0/root")
            .and_then(Value::as_str);
        apps.push(json!({
            "appId": app_id,
            "journeys": status.get("journeys").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
            "untested": status.get("untested").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
            "affectedJourneys": value_or(status.get("affectedJourneys"), json!([])),
            "eligible": value_or(status.pointer("/mergeEligibility/eligible"), Value::Bool(false)),
            "blockingReasons": value_or(status.pointer("/mergeEligibility/blockingReasons"), json!([])),
            "violations": if include_violations {
                root.map(violations_for).unwrap_or_else(|| json!({ "error": "no repository root" }))
            } else { Value::Null },
        }));
    }
    Ok(json!({ "generatedAt": now(), "apps": apps, "fleet": fleet_health() }))
}

pub(crate) fn render_overview(report: &Value) -> String {
    let mut lines = vec![format!(
        "overview {}",
        report
            .get("generatedAt")
            .and_then(Value::as_str)
            .unwrap_or("")
    )];
    for app in report
        .get("apps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let violation = match app.get("violations") {
            Some(Value::Object(value)) => {
                if let Some(error) = value.get("error").and_then(Value::as_str) {
                    format!(" | violations: {error}")
                } else {
                    format!(
                        " | violations: {}",
                        value.get("violations").and_then(Value::as_u64).unwrap_or(0)
                    )
                }
            }
            _ => String::new(),
        };
        lines.push(format!(
            "  {}: journeys {} (untested {}) | eligible: {}{}",
            app.get("appId").and_then(Value::as_str).unwrap_or(""),
            app.get("journeys").and_then(Value::as_u64).unwrap_or(0),
            app.get("untested").and_then(Value::as_u64).unwrap_or(0),
            app.get("eligible")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            violation,
        ));
        for reason in app
            .get("blockingReasons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(3)
            .filter_map(Value::as_str)
        {
            lines.push(format!("    - {reason}"));
        }
    }
    let fleet = report.get("fleet").unwrap_or(&Value::Null);
    if fleet.get("available").and_then(Value::as_bool) == Some(false) {
        lines.push(format!(
            "  fleet: unknown — {}",
            fleet.get("message").and_then(Value::as_str).unwrap_or("")
        ));
        lines.push(format!(
            "         ({} / {} / retryable: {})",
            fleet
                .get("failurePoint")
                .and_then(Value::as_str)
                .unwrap_or(""),
            fleet.get("errorCode").and_then(Value::as_str).unwrap_or(""),
            fleet
                .get("retryable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ));
    } else {
        let live = fleet
            .get("live")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let stale = fleet
            .get("stale")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!(
            "  fleet: live [{}] | stale [{}]",
            if live.is_empty() { "none" } else { &live },
            if stale.is_empty() { "none" } else { &stale },
        ));
    }
    lines.join("\n")
}

pub fn overview(
    harness: &Path,
    app_ids: &[String],
    text: bool,
    include_violations: bool,
) -> Answer {
    let report = overview_value(
        harness,
        (!app_ids.is_empty()).then_some(app_ids),
        include_violations,
    )?;
    if text {
        println!("{}", render_overview(&report));
        Ok(())
    } else {
        print_json(&report)
    }
}
