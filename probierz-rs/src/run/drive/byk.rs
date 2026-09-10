use serde_json::json;
use crate::run::*;
pub(crate) struct BykBroker {
    pub(crate) child: Child,
    pub(crate) directory: PathBuf,
    pub(crate) socket_path: PathBuf,
    pub(crate) recipient: String,
}

impl Drop for BykBroker {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = Command::new("/bin/kill")
                .args(["-TERM", &self.child.id().to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let until = Instant::now() + Duration::from_millis(2000);
            while Instant::now() < until && self.child.try_wait().ok().flatten().is_none() {
                thread::sleep(Duration::from_millis(20));
            }
            if self.child.try_wait().ok().flatten().is_none() {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

pub(crate) fn byk_broker_environment(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut answer = env.clone();
    if !answer.contains_key("SKARBIEC_UNLOCK") {
        let file = answer
            .get("SKARBIEC_UNLOCK_FILE")
            .map(PathBuf::from)
            .or_else(|| {
                answer
                    .get("HOME")
                    .map(|home| PathBuf::from(home).join(".skarbiec-unlock"))
            });
        if let Some(file) = file {
            if let Ok(value) = fs::read_to_string(file) {
                let value = value.trim();
                if !value.is_empty() {
                    answer.insert("SKARBIEC_UNLOCK".into(), value.into());
                }
            }
        }
    }
    answer
}

pub(crate) fn byk_startup_error(message: &str, stderr: &Arc<Mutex<Vec<u8>>>) -> String {
    let safe = stderr
        .lock()
        .ok()
        .map(|raw| {
            let text = String::from_utf8_lossy(&raw);
            tail_chars(&text, TAIL)
                .lines()
                .filter(|line| !line.is_empty())
                .map(|_| "[REDACTED]")
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if safe.is_empty() {
        message.to_string()
    } else {
        format!("{message}\nbroker stderr (sanitized, truncated):\n{safe}")
    }
}

pub(crate) fn valid_byk_recipient(value: &str) -> bool {
    value.len() <= 254
        && Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$")
            .expect("recipient regex")
            .is_match(value)
}

/// The login mailbox this target reads its one-time codes from, and the file
/// that holds the address a resend is sent from. Both are the target's, not a
/// caller's choice: a journey that authenticates a real account has exactly
/// one mailbox.
pub(crate) const BYK_MAILBOX: &str = "byk-ios-login";

/// Where the mailbox broker executable comes from.
///
/// A harness does not build another repository. This used to `cargo build
/// --bin skarbiec-entitlements-router` inside `entitlements-rotator`, which
/// stopped existing on 2026-07-28 when that repository removed its vendored
/// copy of the vault (commit 525f7d6, "Stop being a second source and
/// publisher of Skarbiec"). The journey kept building a binary nobody
/// produced any more and reported it as a build failure, which hid what had
/// actually happened.
///
/// So the broker is now what it always was in truth: an operator-provisioned
/// executable. `BYK_MAILBOX_BROKER` names it, and the refusal says what it
/// must be able to do.
pub(crate) fn byk_broker_binary(
    _harness: &Path,
    env: &BTreeMap<String, String>,
    _timeout_ms: u64,
) -> Result<(PathBuf, PathBuf, BTreeMap<String, String>), String> {
    // The operator's shell counts: a `KEY=VALUE` argument wins, and an
    // exported variable is honoured, exactly as every other condition is.
    let broker_env = byk_broker_environment(&env_snapshot(env));
    let declared = broker_env
        .get("BYK_MAILBOX_BROKER")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!(
            "BYK_MAILBOX_BROKER is required: an executable serving `mailbox-broker --mailbox {BYK_MAILBOX} --socket <path>`, \
`mailbox-probe --mailbox {BYK_MAILBOX}` and `seed-resend <env-file>`. \
No installed product provides it: entitlements-rotator removed its vendored vault binary in 525f7d6 on 2026-07-28 and the surviving copy is the vendored-superset branch of wisent-ai/skarbiec"
        ))?;
    let broker = PathBuf::from(&declared);
    if !broker.is_absolute() {
        return Err(format!(
            "BYK_MAILBOX_BROKER must be an absolute path, not {declared}"
        ));
    }
    let metadata = fs::metadata(&broker)
        .map_err(|error| format!("BYK_MAILBOX_BROKER {declared} cannot be read: {error}"))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!(
            "BYK_MAILBOX_BROKER {declared} is not an executable file"
        ));
    }
    let working = broker
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));
    Ok((broker, working, broker_env))
}

/// Is the login mailbox reachable from here? This is the readiness question
/// `check` asks, and it is the only one that cannot be answered by looking at
/// a file: the broker has to open the mailbox and say so.
pub(crate) fn byk_mailbox_reachable(harness: &Path, env: &BTreeMap<String, String>) -> (bool, String) {
    let (broker, rotator, broker_env) = match byk_broker_binary(harness, env, DEFAULT_TIMEOUT_MS) {
        Ok(parts) => parts,
        Err(reason) => return (false, reason),
    };
    let probe = capture(
        broker.to_string_lossy().as_ref(),
        &[
            "mailbox-probe".into(),
            "--mailbox".into(),
            BYK_MAILBOX.into(),
        ],
        Some(&rotator),
        Some(&broker_env),
        Some(DEFAULT_TIMEOUT_MS),
    );
    if probe.status.is_some_and(|status| status.success()) {
        return (true, String::new());
    }
    let detail = tail_chars(&String::from_utf8_lossy(&probe.stderr), 400)
        .trim()
        .to_string();
    (
        false,
        if detail.is_empty() {
            format!("the {BYK_MAILBOX} mailbox did not answer")
        } else {
            format!("the {BYK_MAILBOX} mailbox did not answer: {detail}")
        },
    )
}

/// Seed the mailbox's resend source and stop. The journey needs an address a
/// resend can come from; seeding it is an operator action on real mail state,
/// so it is its own mode and never a side effect of running the journey.
pub(crate) fn seed_byk_resend(harness: &Path, env: &BTreeMap<String, String>) -> Answer {
    let (broker, rotator, broker_env) = byk_broker_binary(harness, env, DEFAULT_TIMEOUT_MS)
        .map_err(|reason| fail("run.byk.seed", reason))?;
    let source = harness
        .parent()
        .unwrap_or(harness)
        .join("weles")
        .join(".env");
    if !source.exists() {
        return Err(fail(
            "run.byk.seed",
            format!("the resend source {} does not exist", source.display()),
        ));
    }
    let status = Command::new(&broker)
        .args(["seed-resend", source.to_string_lossy().as_ref()])
        .current_dir(&rotator)
        .envs(&broker_env)
        .stdin(Stdio::null())
        .status()
        .map_err(|error| {
            fail(
                "run.byk.seed",
                format!("could not start the Skarbiec mailbox broker: {error}"),
            )
        })?;
    if !status.success() {
        return Err(fail(
            "run.byk.seed",
            format!(
                "seeding the {BYK_MAILBOX} resend source failed with exit {}",
                status.code().unwrap_or(-1)
            ),
        ));
    }
    print_json(&json!({
        "target": "mobile:ios:byk-auth",
        "action": "seed-resend",
        "mailbox": BYK_MAILBOX,
        "source": source,
        "seeded": true,
    }))
}

pub(crate) fn start_byk_broker(
    harness: &Path,
    env: &BTreeMap<String, String>,
    secrets: &[(String, String)],
    stdout_path: &Path,
    stderr_path: &Path,
    timeout_ms: u64,
) -> Result<BykBroker, String> {
    // The broker is provisioned, not built: see `byk_broker_binary`.
    let (broker, rotator, broker_env) = byk_broker_binary(harness, env, timeout_ms)?;
    let _ = (secrets, stdout_path, stderr_path);

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("probierz-byk-auth-{}-{stamp}", std::process::id()));
    fs::create_dir(&directory).map_err(|error| error.to_string())?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let socket_path = directory.join("byk-otp.sock");
    let mut command = Command::new(&broker);
    command
        .args([
            "mailbox-broker",
            "--mailbox",
            "byk-ios-login",
            "--socket",
            socket_path.to_string_lossy().as_ref(),
        ])
        .current_dir(&rotator)
        .envs(&broker_env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            let _ = fs::remove_dir_all(&directory);
            return Err("could not start the Skarbiec mailbox broker".into());
        }
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "could not read the Skarbiec mailbox broker".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "could not read the Skarbiec mailbox broker".to_string())?;
    let stderr_tail = Arc::new(Mutex::new(Vec::new()));
    let stderr_capture = Arc::clone(&stderr_tail);
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut buffer = [0_u8; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            if let Ok(mut tail) = stderr_capture.lock() {
                tail.extend_from_slice(&buffer[..count]);
                if tail.len() > TAIL {
                    let remove = tail.len() - TAIL;
                    tail.drain(..remove);
                }
            }
        }
    });
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line).and_then(|count| {
            if count == 0 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "broker stdout closed",
                ))
            } else {
                Ok(line)
            }
        });
        let _ = sender.send(result);
        let _ = std::io::copy(&mut reader, &mut std::io::sink());
    });
    let mut owner = BykBroker {
        child,
        directory,
        socket_path,
        recipient: String::new(),
    };
    let line = match receiver.recv_timeout(Duration::from_millis(15_000)) {
        Ok(Ok(line)) if line.len() <= 16_384 => line.trim_end_matches(['\r', '\n']).to_string(),
        Ok(Ok(_)) => {
            return Err(byk_startup_error(
                "Skarbiec mailbox broker readiness line was too large",
                &stderr_tail,
            ));
        }
        Ok(Err(_)) => {
            return Err(byk_startup_error(
                "Skarbiec mailbox broker exited before readiness",
                &stderr_tail,
            ));
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(byk_startup_error(
                "timed out waiting for the Skarbiec mailbox broker",
                &stderr_tail,
            ));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(byk_startup_error(
                "Skarbiec mailbox broker exited before readiness",
                &stderr_tail,
            ));
        }
    };
    let readiness: Value = serde_json::from_str(&line).map_err(|_| {
        byk_startup_error(
            "Skarbiec mailbox broker returned invalid readiness JSON",
            &stderr_tail,
        )
    })?;
    if !readiness.is_object()
        || readiness.get("status").and_then(Value::as_str) != Some("ready")
        || readiness.get("mailbox").and_then(Value::as_str) != Some("byk-ios-login")
    {
        return Err(byk_startup_error(
            "Skarbiec mailbox broker returned invalid readiness data",
            &stderr_tail,
        ));
    }
    let socket = readiness
        .get("socket_path")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            byk_startup_error(
                "Skarbiec mailbox broker returned an invalid socket path",
                &stderr_tail,
            )
        })?;
    if !Path::new(socket).is_absolute() || Path::new(socket) != owner.socket_path {
        return Err(byk_startup_error(
            "Skarbiec mailbox broker returned an invalid socket path",
            &stderr_tail,
        ));
    }
    let recipient = readiness
        .get("recipient")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            byk_startup_error(
                "Skarbiec mailbox broker returned an invalid recipient",
                &stderr_tail,
            )
        })?;
    if !valid_byk_recipient(recipient) {
        return Err(byk_startup_error(
            "Skarbiec mailbox broker returned an invalid recipient",
            &stderr_tail,
        ));
    }
    if !fs::metadata(&owner.socket_path)
        .map(|value| value.file_type().is_socket())
        .unwrap_or(false)
    {
        return Err(byk_startup_error(
            "Skarbiec mailbox broker did not create a Unix socket",
            &stderr_tail,
        ));
    }
    owner.recipient = recipient.to_string();
    Ok(owner)
}

/// The byk-auth journey: a real Apple ID login whose one-time code arrives in
/// a real mailbox. The broker owns the mailbox end and hands the suite a
/// socket and the address the code was sent to.
///
/// `local` decides where the XCUITest suite runs: on this machine's simulator,
/// or on the dedicated host through the fleet. The mailbox side is identical
/// either way, because there is only one login account.
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

