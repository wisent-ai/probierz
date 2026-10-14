use crate::mcp::*;
use serde_json::json;
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
    let binary = match probierz_binary() {
        Ok(binary) => binary,
        Err(error) => {
            finish_job_error(&job, error);
            return;
        }
    };
    let mut command = Command::new(binary);
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
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            finish_job_error(&job, format!("cannot run probierz: {error}"));
            return;
        }
    };
    let pid = child.id();
    {
        let Ok(mut job) = job.lock() else {
            terminate_tree(pid);
            return;
        };
        job.child = Some(pid);
        if job.cancel_requested {
            terminate_tree(pid);
        }
    }
    // The run ends when the process exits; a cancel kills it by pid, so no
    // poll is needed to stay cancellable (cli.md rule 8). The wait is said
    // on stderr at both ends, as every Probierz wait on a child is.
    match stado_wait::child_output(child, format!("probierz job runner pid {pid}")) {
        Ok(output) => finish_job(&job, Ok(output.status), output.stdout, output.stderr),
        Err(error) => finish_job(&job, Err(error.to_string()), Vec::new(), Vec::new()),
    }
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
                job.status = if !process_status.success() {
                    job.error = Some(format!(
                        "probierz {}; stderr: {}",
                        crate::failure::ended(&process_status),
                        String::from_utf8_lossy(&stderr)
                    ));
                    "failed"
                } else if result.get("skipped").and_then(Value::as_bool) == Some(true) {
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
                job.error = Some(format!(
                    "probierz returned invalid JSON ({}) — {error}; stderr: {}",
                    crate::failure::ended(&process_status),
                    String::from_utf8_lossy(&stderr)
                ));
                job.status = "failed";
                return;
            }
        }
    }
    let stderr = String::from_utf8_lossy(&stderr);
    job.error = Some(if stderr.trim().is_empty() {
        format!(
            "probierz {} with no message",
            crate::failure::ended(&process_status)
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
        .map_err(|error| format!("resolve artifacts {root} for runId {}: {error}", job.run_id))?;
    let metadata = fs::metadata(&root).map_err(|error| {
        format!(
            "inspect artifacts {} for runId {}: {error}",
            root.display(),
            job.run_id
        )
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "artifacts {} for runId {} are not a directory",
            root.display(),
            job.run_id
        ));
    }
    Ok(root)
}

pub(crate) fn terminate_tree(pid: u32) {
    #[cfg(windows)]
    {
        let mut kill = Command::new("taskkill");
        kill.args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null());
        say_unkilled(&pid.to_string(), stado_wait::output(&mut kill));
    }
    #[cfg(unix)]
    {
        // Without the process table only `pid` itself is known, and its
        // children keep running after the kill; that is said, not assumed away.
        let mut processes: HashMap<u32, Vec<u32>> = HashMap::new();
        match stado_wait::output(Command::new("/bin/ps").args(["-axo", "pid=,ppid="])) {
            Ok(output) if output.status.success() => {
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
            Ok(output) => eprintln!(
                "probierz: /bin/ps answered {} ({}); only pid {pid} is killed and its children may keep running",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            Err(error) => eprintln!(
                "probierz: /bin/ps could not be run ({error}); only pid {pid} is killed and its children may keep running"
            ),
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
        // One KILL to the whole tree; no TERM-then-pause escalation.
        let mut kill = Command::new("/bin/kill");
        kill.arg("-KILL").args(&ids).stdout(Stdio::null());
        say_unkilled(&ids.join(" "), stado_wait::output(&mut kill));
    }
}

/// A job reported cancelled whose processes survive keeps holding what it
/// ran on, so a kill that did not end them all is said with its cause.
fn say_unkilled(ids: &str, killed: std::io::Result<std::process::Output>) {
    match killed {
        Ok(output) if output.status.success() => {}
        Ok(output) => eprintln!(
            "probierz: killing the job's processes {ids} answered {} ({}); some may keep running",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(error) => eprintln!("probierz: the job's processes {ids} could not be killed: {error}"),
    }
}
