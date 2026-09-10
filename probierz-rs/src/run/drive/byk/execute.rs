use serde_json::json;
use crate::run::*;

/// Which fleet host the remote suite is placed on: what the operator asked
/// for, what the run environment declares, or the dedicated Mac otherwise.
pub(crate) fn byk_host_selector(selector: Option<&str>, env: &BTreeMap<String, String>) -> String {
    selector
        .map(str::to_string)
        .or_else(|| env.get("BYK_HOST_SELECTOR").cloned())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "stado:mini".to_string())
}

/// The byk-auth journey: a real Apple ID login whose one-time code arrives in
/// a real mailbox. The broker owns the mailbox end and hands the suite a
/// socket and the address the code was sent to.
///
/// `local` decides where the XCUITest suite runs: on this machine's simulator,
/// or on the dedicated host through the fleet. The mailbox side is identical
/// either way, because there is only one login account.
pub(crate) fn execute_byk(
    local: bool,
    host_selector: &str,
    harness: &Path,
    env: &BTreeMap<String, String>,
    timeout_ms: u64,
    secrets: &[(String, String)],
    stdout_path: &Path,
    stderr_path: &Path,
    started_at: &str,
    artifacts: &Path,
) -> Result<(i32, bool, String, String, Value, Value), Failure> {
    let app = env.get("APP_IOS").map(String::as_str).unwrap_or("");
    let bundle = env.get("BUNDLE_ID").map(String::as_str).unwrap_or("");
    let started = Instant::now();
    let result = (|| -> Result<i32, String> {
        if app != app.trim() || bundle != bundle.trim() {
            return Err("APP_IOS and BUNDLE_ID must not contain surrounding whitespace".into());
        }
        if app.is_empty() == bundle.is_empty() {
            return Err("set exactly one of APP_IOS or BUNDLE_ID".into());
        }
        let broker = start_byk_broker(harness, env, secrets, stdout_path, stderr_path, timeout_ms)?;
        if local {
            // The same suite the `mobile:ios` target runs, told which socket
            // carries the code and which address it was sent to.
            let mut suite_env = env.clone();
            suite_env.insert("PROBIERZ_SPEC".into(), "byk-auth.e2e.ts".into());
            suite_env.insert(
                "BYK_OTP_SOCKET".into(),
                broker.socket_path.display().to_string(),
            );
            suite_env.insert("BYK_TEST_EMAIL".into(), broker.recipient.clone());
            let outcome = execute_suite(
                false,
                host_selector,
                harness,
                "test:mobile:ios",
                &suite_env,
                timeout_ms,
                secrets.to_vec(),
                stdout_path,
                stderr_path,
                "mobile:ios:byk-auth",
                started_at,
                artifacts,
            )
            .map_err(|error| error.detail)?;
            return Ok(outcome.0);
        }
        let outcome = crate::stado::run_remote_byk_auth(crate::stado::RemoteBykRequest {
            root: harness,
            app_path: Path::new(app),
            ios_device: env
                .get("IOS_DEVICE")
                .map(String::as_str)
                .unwrap_or("iPhone 15"),
            ios_version: env.get("IOS_VERSION").map(String::as_str).unwrap_or(""),
            socket_path: &broker.socket_path,
            recipient: &broker.recipient,
            host_selector,
        })
        .map_err(|error| error.detail)?;
        Ok(if outcome.signal.is_some() {
            1
        } else {
            outcome.code.unwrap_or(1)
        })
    })();
    let (exit_code, stderr_tail) = match result {
        Ok(code) => (code, String::new()),
        Err(error) => {
            let safe = redact_text(&format!("byk auth runner: {error}\n"), secrets);
            let _ = append_secure(stderr_path, stamped(&safe).as_bytes());
            (1, safe)
        }
    };
    let performance = json!({
        "schemaVersion": 1,
        "subject": "run-and-app-processes",
        "firstOutputMs": Value::Null,
        "intervalMs": SAMPLE_INTERVAL_MS,
        "peakRssKb": Value::Null,
        "averageCpuPercent": Value::Null,
        "appProcessName": Path::new(app).file_stem().and_then(|name| name.to_str()),
        "appPeakRssKb": Value::Null,
        "appAverageCpuPercent": Value::Null,
        "samples": [],
    });
    let performance_path = artifacts.join("performance.json");
    write_json(&performance_path, &performance)?;
    let mut public = performance;
    public
        .as_object_mut()
        .expect("performance object")
        .insert("file".into(), json!(performance_path));
    public
        .as_object_mut()
        .expect("performance object")
        .shift_remove("samples");
    let diagnostics =
        collect_platform_diagnostics("mobile:ios:byk-auth", env, artifacts, started_at);
    let timed_out = started.elapsed() >= Duration::from_millis(timeout_ms) && exit_code != 0;
    Ok((
        exit_code,
        timed_out,
        String::new(),
        stderr_tail,
        public,
        diagnostics,
    ))
}

