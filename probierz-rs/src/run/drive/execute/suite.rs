use serde_json::json;
use crate::run::*;
pub(crate) struct RunOptions {
    pub(crate) env: BTreeMap<String, String>,
    /// Run the byk-auth suite on this machine instead of the dedicated host.
    pub(crate) local: bool,
    /// The fleet host the remote byk-auth suite is placed on.
    pub(crate) host_selector: String,
    pub(crate) record: bool,
    pub(crate) timeout_ms: u64,
    pub(crate) force: bool,
    pub(crate) spec: Option<String>,
    pub(crate) app_id: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) resource_wait_ms: Option<u64>,
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
        tail = tail_chars(&(tail + &safe), TAIL);
        let _ = append_secure(path, stamped(&safe).as_bytes());
    }
    (tail, first_output_ms)
}

pub(crate) fn execute_suite(
    // Only `mobile:ios:byk-auth` reads these: run its suite here rather than on
    // the fleet, and which fleet host to place it on when it is remote.
    local: bool,
    host_selector: &str,
    harness: &Path,
    script: &str,
    env: &BTreeMap<String, String>,
    timeout_ms: u64,
    secrets: Vec<(String, String)>,
    stdout_path: &Path,
    stderr_path: &Path,
    target_name: &str,
    started_at: &str,
    artifacts: &Path,
) -> Result<(i32, bool, String, String, Value, Value), Failure> {
    if target_name == "mobile:ios:byk-auth" {
        return execute_byk(
            local,
            host_selector,
            harness,
            env,
            timeout_ms,
            &secrets,
            stdout_path,
            stderr_path,
            started_at,
            artifacts,
        );
    }
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
    let pid = child.id();
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
    let mut samples = Vec::new();
    if let Some(sample) = performance_sample(pid, process_name.as_deref()) {
        samples.push(sample);
    }
    let mut next_sample = Instant::now() + Duration::from_millis(SAMPLE_INTERVAL_MS);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= Duration::from_millis(timeout_ms) {
                    timed_out = true;
                    terminate_tree(&mut child, false);
                    thread::sleep(Duration::from_millis(25));
                    if child.try_wait().ok().flatten().is_none() {
                        terminate_tree(&mut child, true);
                    }
                    break child.wait()?;
                }
                if Instant::now() >= next_sample {
                    if let Some(sample) = performance_sample(pid, process_name.as_deref()) {
                        samples.push(sample);
                    }
                    next_sample = Instant::now() + Duration::from_millis(SAMPLE_INTERVAL_MS);
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(Failure::unavailable("run.wait", error.to_string())),
        }
    };
    if let Some(sample) = performance_sample(pid, process_name.as_deref()) {
        samples.push(sample);
    }
    let (safe_out, first_out) = out_thread.join().unwrap_or_default();
    let (safe_err, first_err) = err_thread.join().unwrap_or_default();
    let first_output_ms = match (first_out, first_err) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    };
    let rss: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.get("rssKb").and_then(Value::as_f64))
        .collect();
    let cpu: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.get("cpuPercent").and_then(Value::as_f64))
        .collect();
    let app_rss: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.pointer("/app/rssKb").and_then(Value::as_f64))
        .collect();
    let app_cpu: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.pointer("/app/cpuPercent").and_then(Value::as_f64))
        .collect();
    let average = |values: &[f64]| {
        if values.is_empty() {
            Value::Null
        } else {
            number(values.iter().sum::<f64>() / values.len() as f64)
        }
    };
    let performance = json!({
        "schemaVersion": 1,
        "subject": "run-and-app-processes",
        "firstOutputMs": first_output_ms,
        "intervalMs": SAMPLE_INTERVAL_MS,
        "peakRssKb": rss.iter().copied().max_by(f64::total_cmp).map(number).unwrap_or(Value::Null),
        "averageCpuPercent": average(&cpu),
        "appProcessName": process_name,
        "appPeakRssKb": app_rss.iter().copied().max_by(f64::total_cmp).map(number).unwrap_or(Value::Null),
        "appAverageCpuPercent": average(&app_cpu),
        "samples": samples,
    });
    let performance_path = artifacts.join("performance.json");
    write_json(&performance_path, &performance)?;
    let mut public = performance.clone();
    public
        .as_object_mut()
        .expect("object")
        .insert("file".into(), json!(performance_path));
    public
        .as_object_mut()
        .expect("object")
        .shift_remove("samples");
    let diagnostics = collect_platform_diagnostics(target_name, env, artifacts, started_at);
    Ok((
        status.code().unwrap_or(-1),
        timed_out,
        safe_out,
        safe_err,
        public,
        diagnostics,
    ))
}

