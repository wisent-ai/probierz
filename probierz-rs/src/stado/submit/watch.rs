use crate::stado::*;
use serde_json::json;
/// The terminal states Stado writes on a job record, as it spells them.
#[derive(serde::Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Terminal {
    Failed,
    Cancelled,
    Uploaded,
    Completed,
}

/// Whether a worker started the job: Stado writes `started_at` when one does
/// and leaves it absent or null until then.
fn started(job: &Value) -> bool {
    job.get("started_at").is_some_and(|at| !at.is_null())
}

/// Whether the state `watch_job` answered is one of `wanted`; an unreachable
/// read, or a state outside the terminal ones, is none of them.
fn watched_in(watched: &Value, wanted: &[Terminal]) -> bool {
    match watched
        .get("state")
        .map(|state| serde_json::from_value::<Terminal>(state.clone()))
    {
        Some(Ok(state)) => wanted.contains(&state),
        Some(Err(_not_terminal)) => false,
        None => false,
    }
}

/// Whether a watched job completed: its worker ran and uploaded its result.
pub(crate) fn watched_completed(watched: &Value) -> bool {
    watched_in(watched, &[Terminal::Completed, Terminal::Uploaded])
}

/// Whether a watched job's worker ran to an end that left run evidence:
/// completed or failed. A cancelled or unreachable job left none to fetch.
pub(crate) fn watched_left_evidence(watched: &Value) -> bool {
    watched_in(
        watched,
        &[Terminal::Completed, Terminal::Uploaded, Terminal::Failed],
    )
}

pub(crate) fn terminal_failure(job_id: &str, state: &str, job: &Value) -> Value {
    let reported = job.get("error").filter(|value| !value.is_null());
    // Only the cancelled state, before any worker started, produced no run.
    let cancelled_before_start = matches!(
        serde_json::from_value::<Terminal>(json!(state)),
        Ok(Terminal::Cancelled)
    ) && !started(job);
    // The worker's error as it stated it: its text, or its JSON when it is
    // not text.
    let detail = match reported {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None if cancelled_before_start => {
            "cancelled before the worker started; no run evidence was produced".into()
        }
        None => format!("worker reported {state}"),
    };
    let message = if cancelled_before_start {
        format!("Job {job_id} was cancelled before a worker started; no run evidence was produced.")
    } else {
        format!("Job {job_id} {state} on the remote host: {detail}")
    };
    failure_summary(
        &Failure::new("stado.worker", Code::Unknown, detail),
        message,
    )
}

/// Hold `stado machine status <job> --until terminal` until the job ends.
/// There is no budget, interval or retry: Stado answers when the job's
/// terminal record is written, and a read that fails is reported with
/// Stado's own output as `unreachable`, resumable with `stado resume`.
pub(crate) fn watch_job(job_id: &str) -> Result<Value, Failure> {
    let output = sh(
        STADO_BIN,
        &[
            "machine".into(),
            "status".into(),
            job_id.into(),
            "--until".into(),
            "terminal".into(),
        ],
        None,
    );
    // A read Stado did not answer with `ok: true` JSON is unreachable, and
    // says why: its own output, and the parse cause when it printed no JSON.
    let unreachable = |why: String| {
        let failure = remote_failure(
            "stado.watch",
            &format!("The held status read of job {job_id} failed: {why}"),
            &output,
        );
        json!({
            "state": "unreachable",
            "failure": failure_summary(&failure, format!("The held status read of job {job_id} failed.")),
        })
    };
    let payload: Value = match serde_json::from_str(&output.stdout) {
        Ok(payload) => payload,
        Err(cause) => return Ok(unreachable(format!("its output is not JSON ({cause})"))),
    };
    if payload.get("ok").and_then(Value::as_bool) != Some(true) {
        return Ok(unreachable("Stado did not answer ok".into()));
    }
    let refused = |detail: String| Failure::new("stado.watch", Code::Unknown, detail);
    let job = payload.pointer("/result/job").cloned().ok_or_else(|| {
        refused(format!(
            "stado machine status answered job {job_id} without the job: {payload}"
        ))
    })?;
    let state_text = job
        .get("state")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            refused(format!(
                "stado machine status answered job {job_id} without its state: {job}"
            ))
        })?
        .to_ascii_lowercase();
    let state = serde_json::from_value::<Terminal>(json!(state_text)).map_err(|cause| {
        refused(format!(
            "stado machine status --until terminal answered job {job_id} in state {state_text:?}, which is not terminal ({cause})"
        ))
    })?;
    // A job that resolved no source input carries none: `null`.
    let source = match job.pointer("/resolved_input_artifacts/source") {
        Some(source) => source.clone(),
        None => Value::Null,
    };
    if state == Terminal::Uploaded || state == Terminal::Completed {
        return Ok(json!({
            "state": "completed",
            "job": job,
            "source": source,
            "failure": Value::Null,
        }));
    }
    let failure = terminal_failure(job_id, &state_text, &job);
    let cancelled_before_start = state == Terminal::Cancelled && !started(&job);
    let mut result = json!({
        "state": state_text,
        "source": source,
        "job": job,
        "failure": failure,
    });
    if cancelled_before_start {
        result["evidence"] = json!({
            "required": false, "collected": false, "reason": "cancelled-before-start", "retryable": false,
        });
    }
    Ok(result)
}
