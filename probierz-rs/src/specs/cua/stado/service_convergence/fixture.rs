//! Starting the real convergence fixture, waiting for it, and reading
//! the readiness document it writes.
//!
//! The fixture is Stado's own `service_convergence` integration test,
//! run from the staged source with `cargo test --ignored`. It publishes
//! a readiness JSON document and waits for a stop file, so the journey
//! drives a real host-wide convergence rather than a simulation.

use super::*;

/// The ignored test inside Stado's own suite that stands up the real
/// registry and waits for this journey.
pub(crate) const FIXTURE_TEST: &str = "service_convergence_cua_fixture";

/// How long the fixture may take to build and publish readiness. It
/// compiles Stado from the staged source on a cold target directory.
pub(crate) const READY_TIMEOUT: Duration = Duration::from_secs(180);

/// How long the fixture may take to stop once asked.
pub(crate) const STOP_TIMEOUT: Duration = Duration::from_secs(30);

/// Grace given to a killed fixture before we stop waiting on it.
const KILL_GRACE: Duration = Duration::from_secs(5);

/// Poll interval while waiting for the readiness file.
const READY_POLL: Duration = Duration::from_millis(100);

/// Poll interval while waiting for the process to exit.
const EXIT_POLL: Duration = Duration::from_millis(50);

/// Permissions the fixture's own log files carry: owner read/write
/// only, because the journey's logs quote a dedicated API token path.
#[cfg(unix)]
const LOG_MODE: u32 = 0o600;

pub(crate) fn required(
    context: &specs::Context,
    name: &str,
    message: &str,
) -> Result<String, String> {
    context.optional(name).ok_or_else(|| message.to_string())
}

pub(crate) fn wait_for_file(
    file: &Path,
    child: &mut Child,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while !file.exists() {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!(
                "the real convergence fixture exited before readiness with {}",
                status
                    .code()
                    .map_or_else(|| "signal".to_string(), |code| code.to_string())
            ));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the real convergence fixture wrote no readiness file at {}",
                file.display()
            ));
        }
        thread::sleep(READY_POLL);
    }
    Ok(())
}

pub(crate) fn wait_for_exit(child: &mut Child, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let grace = Instant::now() + KILL_GRACE;
            while Instant::now() < grace {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                thread::sleep(EXIT_POLL);
            }
            return Err("the real convergence fixture did not stop after the CUA journey".into());
        }
        thread::sleep(EXIT_POLL);
    };
    if !status.success() {
        return Err(format!(
            "the real convergence fixture exited with {}",
            status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string())
        ));
    }
    Ok(())
}

pub(crate) fn state_string<'a>(state: &'a Value, field: &str) -> Result<&'a str, String> {
    state
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("the real fixture readiness document has no {field}"))
}

/// Where the fixture writes its readiness, its stop signal, and its
/// logs, all inside this run's artifacts.
pub(crate) struct Control {
    pub(crate) ready: PathBuf,
    pub(crate) stop: PathBuf,
}

/// Spawn the real fixture from the staged Stado source. Returns the
/// child and the control paths the journey reads and writes.
pub(crate) fn spawn(
    context: &specs::Context,
    crate_root: &Path,
) -> Result<(Child, Control), String> {
    let control = context.artifacts.join("stado-service-convergence-fixture");
    fs::create_dir_all(&control).map_err(|error| error.to_string())?;
    let ready = control.join("ready.json");
    let stop = control.join("stop");

    let stdout = log_file(&control.join("fixture.stdout.log"))?;
    let stderr = log_file(&control.join("fixture.stderr.log"))?;

    let child = Command::new("cargo")
        .args([
            "test",
            "--locked",
            "--test",
            "service_convergence",
            FIXTURE_TEST,
            "--",
            "--ignored",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(crate_root)
        .env("STADO_SERVICE_CONVERGENCE_READY", &ready)
        .env("STADO_SERVICE_CONVERGENCE_STOP", &stop)
        .env("CARGO_PROFILE_TEST_DEBUG", "0")
        .env("CARGO_INCREMENTAL", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| error.to_string())?;

    Ok((child, Control { ready, stop }))
}

fn log_file(path: &Path) -> Result<fs::File, String> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(LOG_MODE);
    options.open(path).map_err(|error| error.to_string())
}
