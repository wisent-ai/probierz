use crate::mcp::*;
use serde_json::json;
pub(crate) struct Job {
    pub(crate) run_id: String,
    pub(crate) status: &'static str,
    pub(crate) target: String,
    pub(crate) app_id: String,
    pub(crate) spec: Option<String>,
    pub(crate) record: bool,
    pub(crate) created_at: String,
    pub(crate) started_at: Option<String>,
    pub(crate) completed_at: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) result: Option<Value>,
    pub(crate) cancel_requested: bool,
    pub(crate) child: Option<u32>,
}

#[derive(Default)]
pub(crate) struct Control {
    pub(crate) jobs: Mutex<HashMap<String, Arc<Mutex<Job>>>>,
}

/// `mutex` locked, or the refusal naming what it guards: a lock is only
/// refused when an earlier control call panicked while holding it, and the
/// poison error carries nothing beyond that.
fn locked<'a, T>(mutex: &'a Mutex<T>, what: &str) -> Result<std::sync::MutexGuard<'a, T>, String> {
    mutex.lock().map_err(|_poisoned| {
        format!("{what} is unavailable: an earlier control call panicked while holding it")
    })
}

impl Control {
    pub(crate) fn start(self: &Arc<Self>, args: &Map<String, Value>) -> Result<Value, String> {
        let target = non_empty(args.get("target"), "target")?.to_string();
        // A run names the app it tests; without one it tests Probierz itself.
        let app_id = match args.get("appId") {
            None => "probierz".to_owned(),
            Some(Value::String(app_id)) => app_id.clone(),
            Some(other) => return Err(format!("appId must be text, not {other}")),
        };
        let record = match args.get("record") {
            None => false,
            Some(Value::Bool(record)) => *record,
            Some(other) => return Err(format!("record must be true or false, not {other}")),
        };
        let run_id = new_run_id();
        let job = Arc::new(Mutex::new(Job {
            run_id: run_id.clone(),
            status: "queued",
            target,
            app_id,
            spec: args.get("spec").and_then(Value::as_str).map(str::to_string),
            record,
            created_at: now_iso(),
            started_at: None,
            completed_at: None,
            error: None,
            result: None,
            cancel_requested: false,
            child: None,
        }));
        let answer = public_job(&*locked(&job, "the new job's state")?);
        locked(&self.jobs, "the job table")?.insert(run_id, Arc::clone(&job));

        let control_args = args.clone();
        thread::spawn(move || execute_job(job, control_args));
        Ok(answer)
    }

    pub(crate) fn job(&self, run_id: &str) -> Result<Arc<Mutex<Job>>, String> {
        locked(&self.jobs, "the job table")?
            .get(run_id)
            .cloned()
            .ok_or_else(|| format!("unknown runId: {run_id}"))
    }

    pub(crate) fn status(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let job = self.job(run_id)?;
        let answer = public_job(&*locked(&job, &format!("job {run_id}"))?);
        Ok(answer)
    }

    pub(crate) fn cancel(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let job = self.job(run_id)?;
        let (answer, child) = {
            let mut job = locked(&job, &format!("job {run_id}"))?;
            // A job that has completed, however it ended, has nothing left to cancel.
            if job.completed_at.is_some() {
                return answered(public_job(&job), "cancelRequested", Value::Bool(false));
            }
            job.cancel_requested = true;
            (
                answered(public_job(&job), "cancelRequested", Value::Bool(true))?,
                job.child.clone(),
            )
        };
        if let Some(child) = child {
            terminate_tree(child);
        }
        Ok(answer)
    }

    pub(crate) fn result(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let job = self.job(run_id)?;
        let job = locked(&job, &format!("job {run_id}"))?;
        answered(public_job(&job), "result", json!(job.result))
    }

    pub(crate) fn list_artifacts(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let job = self.job(run_id)?;
        let root = artifact_root(&job)?;
        let mut pending = vec![root.clone()];
        let mut artifacts = Vec::new();
        while let Some(directory) = pending.pop() {
            let entries = fs::read_dir(&directory).map_err(|error| error.to_string())?;
            for entry in entries {
                let entry = entry.map_err(|error| error.to_string())?;
                let metadata = entry.file_type().map_err(|error| error.to_string())?;
                if metadata.is_dir() {
                    pending.push(entry.path());
                } else if metadata.is_file() {
                    let bytes = entry.metadata().map_err(|error| error.to_string())?.len();
                    let file = entry
                        .path()
                        .strip_prefix(&root)
                        .map_err(|error| error.to_string())?
                        .to_string_lossy()
                        .into_owned();
                    artifacts.push(json!({ "file": file, "bytes": bytes }));
                }
            }
        }
        artifacts.sort_by(|left, right| {
            left.get("file")
                .and_then(Value::as_str)
                .cmp(&right.get("file").and_then(Value::as_str))
        });
        Ok(json!({ "runId": run_id, "artifacts": artifacts }))
    }

    pub(crate) fn get_artifact(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let relative = non_empty(args.get("file"), "file")
            .map_err(|error| format!("file must be a non-empty relative path: {error}"))?;
        let job = self.job(run_id)?;
        let root = artifact_root(&job)?;
        let relative_path = Path::new(relative);
        if relative_path.is_absolute()
            || relative_path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err("artifact path escapes the run directory".to_string());
        }
        let lexical = root.join(relative_path);
        let metadata = fs::symlink_metadata(&lexical).map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!("artifact is not a file: {relative}"));
        }
        let file = fs::canonicalize(&lexical).map_err(|error| error.to_string())?;
        if file != root && !file.starts_with(&root) {
            return Err("artifact path escapes the run directory".to_string());
        }
        let content = fs::read(&file).map_err(|error| error.to_string())?;
        let file = lexical
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .into_owned();
        Ok(json!({
            "runId": run_id,
            "file": file,
            "bytes": metadata.len(),
            "encoding": "base64",
            "content": BASE64.encode(content),
        }))
    }

    /// Cancels every job that has not finished, even when a panic poisoned
    /// the job table or a job: shutdown must leave no run behind.
    pub(crate) fn shutdown(&self) {
        let jobs = match self.jobs.lock() {
            Ok(jobs) => jobs.values().cloned().collect::<Vec<_>>(),
            Err(poisoned) => poisoned.into_inner().values().cloned().collect::<Vec<_>>(),
        };
        for job in jobs {
            let mut job = match job.lock() {
                Ok(job) => job,
                Err(poisoned) => poisoned.into_inner(),
            };
            if job.completed_at.is_some() {
                continue;
            }
            job.cancel_requested = true;
            if let Some(child) = job.child.clone() {
                drop(job);
                terminate_tree(child);
            }
        }
    }
}

/// `answer`, one job as `public_job` writes it, with `field` set to `value`.
/// A job written as anything but an object is refused naming what it was.
fn answered(answer: Value, field: &str, value: Value) -> Result<Value, String> {
    match answer {
        Value::Object(mut object) => {
            object.insert(field.into(), value);
            Ok(Value::Object(object))
        }
        other => Err(format!(
            "a job was answered as {other}, not an object, so its {field} cannot be set"
        )),
    }
}
