use serde_json::json;
use crate::*;
pub(crate) fn new_run_id() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let uuid = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}-{}",
        now_iso().replace(':', "-").replace('.', "-"),
        &uuid[0..8],
        &uuid[8..12],
        &uuid[12..16],
        &uuid[16..20],
        &uuid[20..32]
    )
}

pub(crate) fn public_job(job: &Job) -> Value {
    let artifacts_dir = job
        .result
        .as_ref()
        .and_then(|result| result.get("artifactsDir"))
        .cloned()
        .unwrap_or(Value::Null);
    json!({
        "runId": job.run_id,
        "status": job.status,
        "target": job.target,
        "appId": job.app_id,
        "spec": job.spec,
        "record": job.record,
        "createdAt": job.created_at,
        "startedAt": job.started_at,
        "completedAt": job.completed_at,
        "error": job.error,
        "artifactsDir": artifacts_dir,
    })
}

pub(crate) fn execute_job(job: Arc<Mutex<Job>>, mut args: Map<String, Value>) {
    {
        let Ok(mut job) = job.lock() else {
            return;
        };
        job.status = "running";
        job.started_at = Some(now_iso());
        if job.cancel_requested {
            job.status = "canceled";
            job.completed_at = Some(now_iso());
            return;
        }
    }

    let analyze = args
        .remove("analyze")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let arguments = match route("probierz_run", &args) {
        Ok(mut arguments) => {
            if !analyze {
                arguments.push("--no-analyze".to_string());
            }
            arguments
        }
        Err(error) => {
            finish_job_error(&job, error);
            return;
        }
    };
    let mut command = Command::new(probierz_binary());
    command
        .arg("--harness")
        .arg(harness_root())
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            finish_job_error(&job, format!("cannot run probierz: {error}"));
            return;
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(child));
    {
        let Ok(mut job) = job.lock() else {
            terminate_tree(&child);
            return;
        };
        job.child = Some(Arc::clone(&child));
        if job.cancel_requested {
            terminate_tree(&child);
        }
    }
    let stdout_reader = thread::spawn(move || read_pipe(stdout));
    let stderr_reader = thread::spawn(move || read_pipe(stderr));
    let status = loop {
        let waited = child
            .lock()
            .map_err(|_| "run process state unavailable".to_string())
            .and_then(|mut child| child.try_wait().map_err(|error| error.to_string()));
        match waited {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => break Err(error),
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    finish_job(&job, status, stdout, stderr);
}

pub(crate) fn read_pipe(pipe: Option<impl Read>) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut bytes);
    }
    bytes
}

pub(crate) fn finish_job(
    job: &Arc<Mutex<Job>>,
    status: Result<ExitStatus, String>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
) {
    let Ok(mut job) = job.lock() else {
        return;
    };
    job.child = None;
    job.completed_at = Some(now_iso());
    if job.cancel_requested {
        job.status = "canceled";
        return;
    }
    let process_status = match status {
        Ok(status) => status,
        Err(error) => {
            job.status = "failed";
            job.error = Some(error);
            return;
        }
    };
    if !stdout.is_empty() {
        match serde_json::from_slice::<Value>(&stdout) {
            Ok(result) => {
                job.status = if result.get("skipped").and_then(Value::as_bool) == Some(true) {
                    "blocked"
                } else if result.get("canceled").and_then(Value::as_bool) == Some(true) {
                    "canceled"
                } else if result.get("passed").and_then(Value::as_bool) == Some(true) {
                    "passed"
                } else {
                    "failed"
                };
                job.result = Some(result);
                return;
            }
            Err(error) => {
                job.error = Some(format!("probierz returned invalid JSON: {error}"));
                job.status = "failed";
                return;
            }
        }
    }
    let stderr = String::from_utf8_lossy(&stderr);
    job.error = Some(if stderr.trim().is_empty() {
        format!(
            "exit {}",
            process_status
                .code()
                .map_or_else(|| "null".to_string(), |code| code.to_string())
        )
    } else {
        stderr.trim().to_string()
    });
    job.status = "failed";
}

pub(crate) fn finish_job_error(job: &Arc<Mutex<Job>>, error: String) {
    if let Ok(mut job) = job.lock() {
        job.status = if job.cancel_requested {
            "canceled"
        } else {
            "failed"
        };
        job.error = Some(error);
        job.completed_at = Some(now_iso());
    }
}

pub(crate) fn artifact_root(job: &Arc<Mutex<Job>>) -> Result<PathBuf, String> {
    let job = job
        .lock()
        .map_err(|_| "control state unavailable".to_string())?;
    let root = job
        .result
        .as_ref()
        .and_then(|result| result.get("artifactsDir"))
        .and_then(Value::as_str)
        .filter(|root| !root.is_empty())
        .ok_or_else(|| format!("artifacts unavailable for runId: {}", job.run_id))?;
    let root = fs::canonicalize(root)
        .map_err(|_| format!("artifacts unavailable for runId: {}", job.run_id))?;
    if !root.is_dir() {
        return Err(format!("artifacts unavailable for runId: {}", job.run_id));
    }
    Ok(root)
}

pub(crate) fn terminate_tree(child: &Arc<Mutex<Child>>) {
    let Ok(mut child) = child.lock() else {
        return;
    };
    let pid = child.id();
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        let mut processes: HashMap<u32, Vec<u32>> = HashMap::new();
        if let Ok(output) = Command::new("/bin/ps")
            .args(["-axo", "pid=,ppid="])
            .output()
        {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let mut columns = line.split_whitespace();
                if let (Some(process), Some(parent)) = (columns.next(), columns.next()) {
                    if let (Ok(process), Ok(parent)) =
                        (process.parse::<u32>(), parent.parse::<u32>())
                    {
                        processes.entry(parent).or_default().push(process);
                    }
                }
            }
        }
        fn descendants(pid: u32, processes: &HashMap<u32, Vec<u32>>, output: &mut Vec<u32>) {
            for child in processes.get(&pid).into_iter().flatten() {
                descendants(*child, processes, output);
            }
            output.push(pid);
        }
        let mut tree = Vec::new();
        descendants(pid, &processes, &mut tree);
        let ids = tree.iter().map(u32::to_string).collect::<Vec<_>>();
        let _ = Command::new("/bin/kill")
            .arg("-TERM")
            .args(&ids)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        thread::sleep(Duration::from_millis(100));
        let _ = Command::new("/bin/kill")
            .arg("-KILL")
            .args(&ids)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

