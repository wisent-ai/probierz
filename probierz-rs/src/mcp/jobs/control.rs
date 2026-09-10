use serde_json::json;
use crate::*;
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
    pub(crate) child: Option<Arc<Mutex<Child>>>,
}

#[derive(Default)]
pub(crate) struct Control {
    pub(crate) jobs: Mutex<HashMap<String, Arc<Mutex<Job>>>>,
}

impl Control {
    pub(crate) fn start(self: &Arc<Self>, args: &Map<String, Value>) -> Result<Value, String> {
        let target = non_empty(args.get("target"), "target")?.to_string();
        let run_id = new_run_id();
        let job = Arc::new(Mutex::new(Job {
            run_id: run_id.clone(),
            status: "queued",
            target,
            app_id: args
                .get("appId")
                .and_then(Value::as_str)
                .unwrap_or("probierz")
                .to_string(),
            spec: args.get("spec").and_then(Value::as_str).map(str::to_string),
            record: args.get("record").and_then(Value::as_bool).unwrap_or(false),
            created_at: now_iso(),
            started_at: None,
            completed_at: None,
            error: None,
            result: None,
            cancel_requested: false,
            child: None,
        }));
        let answer = {
            let job = job
                .lock()
                .map_err(|_| "control state unavailable".to_string())?;
            public_job(&job)
        };
        self.jobs
            .lock()
            .map_err(|_| "control state unavailable".to_string())?
            .insert(run_id, Arc::clone(&job));

        let control_args = args.clone();
        thread::spawn(move || execute_job(job, control_args));
        Ok(answer)
    }

    pub(crate) fn job(&self, run_id: &str) -> Result<Arc<Mutex<Job>>, String> {
        self.jobs
            .lock()
            .map_err(|_| "control state unavailable".to_string())?
            .get(run_id)
            .cloned()
            .ok_or_else(|| format!("unknown runId: {run_id}"))
    }

    pub(crate) fn status(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let job = self.job(run_id)?;
        let answer = {
            let job = job
                .lock()
                .map_err(|_| "control state unavailable".to_string())?;
            public_job(&job)
        };
        Ok(answer)
    }

    pub(crate) fn cancel(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let job = self.job(run_id)?;
        let (answer, child) = {
            let mut job = job
                .lock()
                .map_err(|_| "control state unavailable".to_string())?;
            if matches!(job.status, "passed" | "failed" | "blocked" | "canceled") {
                let mut answer = public_job(&job);
                answer
                    .as_object_mut()
                    .expect("job answer")
                    .insert("cancelRequested".into(), Value::Bool(false));
                return Ok(answer);
            }
            job.cancel_requested = true;
            let mut answer = public_job(&job);
            answer
                .as_object_mut()
                .expect("job answer")
                .insert("cancelRequested".into(), Value::Bool(true));
            (answer, job.child.clone())
        };
        if let Some(child) = child {
            terminate_tree(&child);
        }
        Ok(answer)
    }

    pub(crate) fn result(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let job = self.job(run_id)?;
        let job = job
            .lock()
            .map_err(|_| "control state unavailable".to_string())?;
        let mut answer = public_job(&job);
        answer
            .as_object_mut()
            .expect("job answer")
            .insert("result".into(), job.result.clone().unwrap_or(Value::Null));
        Ok(answer)
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
            .map_err(|_| "file must be a non-empty relative path".to_string())?;
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
        if metadata.len() > MAX_ARTIFACT_BYTES {
            return Err(format!(
                "artifact exceeds {MAX_ARTIFACT_BYTES} byte inline limit"
            ));
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

    pub(crate) fn shutdown(&self) {
        let jobs = self
            .jobs
            .lock()
            .map(|jobs| jobs.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for job in jobs {
            let child = job.lock().ok().and_then(|mut job| {
                if matches!(job.status, "queued" | "running") {
                    job.cancel_requested = true;
                    job.child.clone()
                } else {
                    None
                }
            });
            if let Some(child) = child {
                terminate_tree(&child);
            }
        }
    }
}

