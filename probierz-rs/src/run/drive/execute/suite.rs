use crate::run::*;
use serde_json::json;
pub(crate) struct RunOptions {
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) record: bool,
    pub(crate) force: bool,
    pub(crate) spec: Option<String>,
    pub(crate) app_id: Option<String>,
    pub(crate) kind: Option<String>,
}

pub(crate) fn drain_run_stream<R: Read>(
    mut stream: R,
    path: &Path,
    secrets: &[(String, String)],
    run_started: DateTime<Utc>,
) -> (String, Option<u64>) {
    let mut tail = String::new();
    let mut first_output_ms = None;
    let mut buffer = [0u8; 8192];
    loop {
        let count = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        if first_output_ms.is_none() {
            first_output_ms = Some(
                (Utc::now().timestamp_millis() - run_started.timestamp_millis()).max(0) as u64,
            );
        }
        let safe = redact_text(&String::from_utf8_lossy(&buffer[..count]), secrets);
        tail.push_str(&safe);
        let _ = append_secure(path, stamped(&safe).as_bytes());
    }
    (tail, first_output_ms)
}

/// Resource accounting the kernel keeps for the children this process has
/// waited for: CPU time and peak resident size. Read once after the runner
/// exits, so no timer samples the process while it runs.
#[repr(C)]
struct Timeval {
    tv_sec: i64,
    #[cfg(target_os = "macos")]
    tv_usec: i32,
    #[cfg(not(target_os = "macos"))]
    tv_usec: i64,
}

#[repr(C)]
struct Rusage {
    ru_utime: Timeval,
    ru_stime: Timeval,
    ru_maxrss: i64,
    ru_ixrss: i64,
    ru_idrss: i64,
    ru_isrss: i64,
    ru_minflt: i64,
    ru_majflt: i64,
    ru_nswap: i64,
    ru_inblock: i64,
    ru_oublock: i64,
    ru_msgsnd: i64,
    ru_msgrcv: i64,
    ru_nsignals: i64,
    ru_nvcsw: i64,
    ru_nivcsw: i64,
}

const RUSAGE_CHILDREN: i32 = -1;

/// `(cpu seconds, peak resident KiB)` of the waited-for children, or `None`
/// when the kernel refuses the read.
fn children_accounting() -> Option<(f64, f64)> {
    extern "C" {
        fn getrusage(who: i32, usage: *mut Rusage) -> i32;
    }
    let mut usage = std::mem::MaybeUninit::<Rusage>::uninit();
    // SAFETY: getrusage writes a full rusage into the buffer it is handed.
    let usage = unsafe {
        if getrusage(RUSAGE_CHILDREN, usage.as_mut_ptr()) != 0 {
            return None;
        }
        usage.assume_init()
    };
    let seconds =
        |time: &Timeval| time.tv_sec as f64 + f64::from(time.tv_usec as i32) / 1_000_000.0;
    let cpu = seconds(&usage.ru_utime) + seconds(&usage.ru_stime);
    // macOS reports ru_maxrss in bytes, Linux in kibibytes.
    let peak_kib = if cfg!(target_os = "macos") {
        usage.ru_maxrss as f64 / 1024.0
    } else {
        usage.ru_maxrss as f64
    };
    Some((cpu, peak_kib))
}

pub(crate) fn execute_suite(
    harness: &Path,
    script: &str,
    env: &BTreeMap<String, String>,
    secrets: Vec<(String, String)>,
    stdout_path: &Path,
    stderr_path: &Path,
    target_name: &str,
    started_at: &str,
    artifacts: &Path,
) -> Result<(i32, String, String, Value, Value), Failure> {
    let mut command = Command::new("npm");
    command
        .args(["run", script])
        .current_dir(harness)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        Failure::config(
            "run.spawn",
            format!("Starting the {target_name} runner failed: {error}"),
        )
    })?;
    let child_out = child.stdout.take().expect("piped stdout");
    let child_err = child.stderr.take().expect("piped stderr");
    let out_path = stdout_path.to_path_buf();
    let err_path = stderr_path.to_path_buf();
    let out_secrets = secrets.clone();
    let err_secrets = secrets;
    let run_started = DateTime::parse_from_rfc3339(started_at)
        .map(|date| date.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let out_thread =
        thread::spawn(move || drain_run_stream(child_out, &out_path, &out_secrets, run_started));
    let err_thread =
        thread::spawn(move || drain_run_stream(child_err, &err_path, &err_secrets, run_started));

    let started = Instant::now();
    let process_name = {
        let app_path = if matches!(target_name, "desktop:mac" | "desktop:cua") {
            env.get("MAC_APP_PATH")
        } else {
            env.get("APP_IOS")
        };
        app_path
            .and_then(|path| Path::new(path).file_stem())
            .and_then(|name| name.to_str())
            .map(str::to_string)
    };
    // The runner runs to its own end. Its exit status is its result; nothing
    // here decides it took too long.
    let status = child
        .wait()
        .map_err(|error| Failure::unavailable("run.wait", error.to_string()))?;
    let wall = started.elapsed().as_secs_f64();
    let (safe_out, first_out) = out_thread.join().unwrap_or_default();
    let (safe_err, first_err) = err_thread.join().unwrap_or_default();
    let first_output_ms = match (first_out, first_err) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    };
    let accounting = children_accounting();
    let performance = json!({
        "schemaVersion": 2,
        "subject": "run-process-exit-accounting",
        "firstOutputMs": first_output_ms,
        "wallSeconds": number(wall),
        "cpuSeconds": accounting.map(|(cpu, _)| number(cpu)).unwrap_or(Value::Null),
        "averageCpuPercent": accounting
            .filter(|_| wall > 0.0)
            .map(|(cpu, _)| number(cpu / wall * 100.0))
            .unwrap_or(Value::Null),
        "peakRssKb": accounting.map(|(_, rss)| number(rss)).unwrap_or(Value::Null),
        "appProcessName": process_name,
    });
    let performance_path = artifacts.join("performance.json");
    write_json(&performance_path, &performance)?;
    let mut public = performance.clone();
    public
        .as_object_mut()
        .expect("object")
        .insert("file".into(), json!(performance_path));
    let diagnostics = collect_platform_diagnostics(target_name, env, artifacts, started_at);
    Ok((
        status.code().unwrap_or(-1),
        safe_out,
        safe_err,
        public,
        diagnostics,
    ))
}
