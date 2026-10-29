use crate::stado::*;

/// The shape of a Stado job id as Stado writes it (stado-rs
/// queue/submit/request/identity.rs): `job-` and lowercase hex, `x` standing
/// for one hex digit.
const JOB_ID_SHAPE: &str = "job-xxxxxxxxxxxxxxxxxxxxxxxx";
const JOB_ID_PREFIX: &str = "job-";

/// The manifest fields a resumed run reports; one the manifest does not state
/// is reported as null, so the reader sees which the run left out.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RunFields {
    run_id: Option<Value>,
    app_id: Option<Value>,
    target: Option<Value>,
}

pub(crate) fn resume_remote_run(
    harness: &Path,
    job_id: Option<&str>,
    host_name: &str,
) -> Result<Value, Failure> {
    let job_id = match job_id {
        Some(job_id) if safe_job_identifier(job_id) => job_id,
        Some(_) | None => {
            return Err(Failure::config(
                "stado.watch",
                "Resuming remote evidence needs a valid existing Stado job ID.",
            ))
        }
    };
    host(host_name, "stado.watch")?;
    let watched = watch_job(job_id)?;
    let left_evidence = watched_left_evidence(&watched);
    let completed = watched_completed(&watched);
    let mut result = serde_json::Map::new();
    result.insert("host".into(), Value::String(host_name.into()));
    result.insert("jobId".into(), Value::String(job_id.into()));
    result.insert("submitted".into(), Value::Bool(false));
    match watched {
        Value::Object(watched) => result.extend(watched),
        other => {
            return Err(Failure::invalid(
                "stado.watch",
                format!("the watch of job {job_id} answered {other}, not an object"),
            ))
        }
    }
    if !left_evidence {
        return Ok(Value::Object(result));
    }
    let retained = fetch_run_evidence(harness, job_id)?;
    if let Some(path) = &retained.results_dir {
        result.insert(
            "resultsDir".into(),
            Value::String(path.display().to_string()),
        );
    }
    if let Some(error) = &retained.artifact_error {
        result.insert("artifactError".into(), error.clone());
    }
    if let Some(run) = &retained.manifest {
        let fields: RunFields = serde_json::from_value(run.clone()).map_err(|cause| {
            Failure::invalid(
                "stado.download",
                format!("job {job_id}: run manifest: {cause}"),
            )
        })?;
        match serde_json::to_value(fields) {
            Ok(Value::Object(fields)) => result.extend(fields),
            Ok(other) => unreachable!("run fields serialize as an object, not {other}"),
            Err(cause) => unreachable!("run fields of JSON values serialize: {cause}"),
        }
        let authored = restore_remote_authoring(
            harness,
            job_id,
            &retained,
            run.get("appId").and_then(Value::as_str),
            completed && read_author_submission(harness, job_id).is_some(),
        )?;
        if let Some(Value::Object(authored)) = authored {
            result.extend(authored);
        }
    } else if completed {
        let detail = match &retained.artifact_error {
            Some(error) => format!("artifact_error={error}"),
            None => "the run left no manifest and no artifact error".into(),
        };
        result.insert("state".into(), Value::String("evidence-unavailable".into()));
        result.insert("failure".into(), missing_evidence(job_id, &detail));
    }
    Ok(Value::Object(result))
}

/// Whether `value` is a job id of the shape Stado writes.
pub(crate) fn canonical_job_id(value: &str) -> bool {
    value.len() == JOB_ID_SHAPE.len()
        && value.strip_prefix(JOB_ID_PREFIX).is_some_and(|hex| {
            hex.bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
}

/// Whether `value` can name a job on the command line: it starts with a
/// letter or digit and holds only those and `.`, `_`, `-`.
pub(crate) fn safe_job_identifier(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
