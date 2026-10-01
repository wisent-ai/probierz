use serde_json::json;
use crate::stado::*;
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
    let payload: Option<Value> = serde_json::from_str(&output.stdout).ok();
    let answered = payload
        .as_ref()
        .and_then(|value| value.get("ok"))
        .and_then(Value::as_bool)
        == Some(true);
    if !answered {
        let failure = remote_failure(
            "stado.watch",
            &format!("The held status read of job {job_id} failed"),
            &output,
        );
        return Ok(json!({
            "state": "unreachable",
            "failure": failure_summary(&failure, format!("The held status read of job {job_id} failed.")),
        }));
    }
    let job = payload
        .as_ref()
        .and_then(|value| value.pointer("/result/job"))
        .cloned()
        .unwrap_or(Value::Null);
    let state = job
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let source = job
        .pointer("/resolved_input_artifacts/source")
        .cloned()
        .unwrap_or(Value::Null);
    match state.as_str() {
        "failed" | "cancelled" => {
            let failure = terminal_failure(job_id, &state, &job);
            let before_start = job.get("started_at").map(Value::is_null).unwrap_or(true);
            let mut result = json!({
                "state": state,
                "source": source,
                "job": job,
                "failure": failure,
            });
            if state == "cancelled" && before_start {
                result.as_object_mut().expect("object").insert("evidence".into(), json!({
                    "required": false, "collected": false, "reason": "cancelled-before-start", "retryable": false,
                }));
            }
            Ok(result)
        }
        "uploaded" | "completed" => Ok(json!({
            "state": "completed",
            "job": job,
            "source": source,
            "failure": Value::Null,
        })),
        other => Err(Failure::new(
            "stado.watch",
            Code::Unknown,
            format!("stado machine status --until terminal answered job {job_id} in state {other:?}, which is not terminal"),
        )),
    }
}

