use serde_json::json;
use crate::stado::*;
pub(crate) fn budget_from_job(job: &Value) -> Option<u64> {
    let command = job.get("command")?.as_str()?;
    let prefix = format!("{WATCH_BUDGET_ENV}=");
    command
        .split_ascii_whitespace()
        .find_map(|part| part.strip_prefix(&prefix))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
}

pub(crate) fn timestamp_millis(value: Option<&Value>) -> Option<i64> {
    value
        .and_then(Value::as_str)
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.timestamp_millis())
}

pub(crate) fn terminal_failure(job_id: &str, state: &str, job: &Value) -> Value {
    let reported = job.get("error").filter(|value| !value.is_null());
    let detail = if let Some(reported) = reported {
        reported
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| reported.to_string())
    } else if state == "cancelled" && job.get("started_at").map(Value::is_null).unwrap_or(true) {
        "cancelled before the worker started; no run evidence was produced".into()
    } else {
        format!("worker reported {state}")
    };
    let message = if state == "cancelled"
        && job.get("started_at").map(Value::is_null).unwrap_or(true)
    {
        format!("Job {job_id} was cancelled before a worker started; no run evidence was produced.")
    } else {
        format!("Job {job_id} {state} on the remote host: {detail}")
    };
    failure_summary(
        &Failure::new("stado.worker", Code::Unknown, detail),
        message,
    )
}

pub(crate) fn watch_job(
    harness: &Path,
    job_id: &str,
    selected: &discovery::Host,
    requested_budget: Option<u64>,
) -> Result<Value, Failure> {
    let now = Utc::now().timestamp_millis();
    let mut watch_budget = requested_budget;
    let mut budget_source = if requested_budget.is_some() {
        "submitted"
    } else {
        "original run"
    };
    let mut deadline = now.saturating_add(requested_budget.unwrap_or(SETUP_STEP_TIMEOUT_MS) as i64);
    let mut resolved_budget = false;
    let mut anchored_started_at = None;
    let mut failures = 0_usize;
    let mut last_job = Value::Null;
    let mut last_answered = false;
    loop {
        let current = Utc::now().timestamp_millis();
        if current >= deadline {
            break;
        }
        let remaining =
            Duration::from_millis((deadline - current).max(1) as u64).min(STATUS_TIMEOUT);
        let output = sh(
            STADO_BIN,
            &["machine".into(), "status".into(), job_id.into()],
            None,
            Some(selected),
            Some(remaining),
        );
        let payload: Option<Value> = serde_json::from_str(&output.stdout).ok();
        let answered = payload
            .as_ref()
            .and_then(|value| value.get("ok"))
            .and_then(Value::as_bool)
            == Some(true);
        last_answered = answered;
        let job = payload
            .as_ref()
            .and_then(|value| value.pointer("/result/job"))
            .cloned()
            .unwrap_or(Value::Null);
        if answered {
            last_job = job.clone();
        }
        let state = job
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        if answered
            && !resolved_budget
            && !matches!(
                state.as_str(),
                "failed" | "cancelled" | "completed" | "uploaded"
            )
        {
            let saved = budget_from_job(&job);
            watch_budget = saved.or(requested_budget);
            if watch_budget.is_none() {
                // Older submissions did not carry a budget.  A local authoring
                // receipt gives an exact application fallback without guessing.
                let app_id = read_author_submission(harness, job_id).and_then(|receipt| {
                    receipt
                        .get("appId")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
                watch_budget = match app_id {
                    Some(app_id) => Some(conservative_watch_budget(harness, &app_id)?),
                    None => None,
                };
            }
            let budget = watch_budget.ok_or_else(|| {
                Failure::config(
                    "stado.watch",
                    "The original run contract cannot be recovered from this job.",
                )
            })?;
            budget_source = if saved.is_some() {
                "saved submission"
            } else if requested_budget.is_some() {
                "submitted"
            } else {
                "original application fallback"
            };
            let created = timestamp_millis(job.get("created_at"))
                .unwrap_or_else(|| Utc::now().timestamp_millis());
            deadline = created.saturating_add(budget as i64);
            resolved_budget = true;
        }
        if answered {
            if let Some(started) = timestamp_millis(job.get("started_at")) {
                if anchored_started_at != Some(started) {
                    deadline = started
                        .saturating_add(watch_budget.unwrap_or(SETUP_STEP_TIMEOUT_MS) as i64);
                    anchored_started_at = Some(started);
                }
            }
            failures = 0;
        } else {
            failures += 1;
            if failures >= STATUS_FAILURE_TOLERANCE {
                let failure = remote_failure(
                    "stado.watch",
                    &format!("The stado queue stopped answering about job {job_id}"),
                    &output,
                );
                return Ok(json!({
                    "state": "unreachable",
                    "watchBudgetMs": watch_budget,
                    "failure": failure_summary(&failure, format!("The stado queue stopped answering about job {job_id}.")),
                }));
            }
        }
        if matches!(state.as_str(), "failed" | "cancelled") {
            let mut result = json!({
                "state": state,
                "source": job.pointer("/resolved_input_artifacts/source").cloned().unwrap_or(Value::Null),
                "job": job,
                "watchBudgetMs": watch_budget,
                "failure": terminal_failure(job_id, &state, &last_job),
            });
            if state == "cancelled"
                && last_job
                    .get("started_at")
                    .map(Value::is_null)
                    .unwrap_or(true)
            {
                result.as_object_mut().expect("object").insert("evidence".into(), json!({
                    "required": false, "collected": false, "reason": "cancelled-before-start", "retryable": false,
                }));
            }
            return Ok(result);
        }
        if matches!(state.as_str(), "uploaded" | "completed") {
            return Ok(json!({
                "state": "completed",
                "job": job,
                "source": last_job.pointer("/resolved_input_artifacts/source").cloned().unwrap_or(Value::Null),
                "watchBudgetMs": watch_budget,
                "failure": Value::Null,
            }));
        }
        let remaining = deadline - Utc::now().timestamp_millis();
        if remaining > 0 {
            thread::sleep(WATCH_INTERVAL.min(Duration::from_millis(remaining as u64)));
        }
    }
    let state = last_job
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("running")
        .to_ascii_lowercase();
    let budget = watch_budget.unwrap_or(SETUP_STEP_TIMEOUT_MS);
    let message = if last_answered {
        format!("Probierz stopped watching job {job_id} after its {budget}ms {budget_source} budget; Stado was still answering and the job remains {state}. Resume this job to continue waiting.")
    } else {
        format!("Probierz stopped watching job {job_id} after its {budget}ms {budget_source} budget; the last status read did not answer, but the queue-unreachable threshold was not reached. Resume this job to continue waiting.")
    };
    Ok(json!({
        "state": "watch-expired",
        "job": last_job,
        "source": last_job.pointer("/resolved_input_artifacts/source").cloned().unwrap_or(Value::Null),
        "watchBudgetMs": budget,
        "failure": failure_summary(&Failure::new("stado.watch", Code::Unknown, &message), message),
    }))
}

