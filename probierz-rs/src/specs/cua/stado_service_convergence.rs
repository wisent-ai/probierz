use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

use regex::Regex;
use serde_json::Value;

use crate::{cua::App, specs};

use super::stado_console as console;

const FIXTURE_TEST: &str = "service_convergence_cua_fixture";

fn required(context: &specs::Context, name: &str, message: &str) -> Result<String, String> {
    context.optional(name).ok_or_else(|| message.to_string())
}

fn wait_for_file(file: &Path, child: &mut Child, timeout: Duration) -> Result<(), String> {
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
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let grace = Instant::now() + Duration::from_secs(5);
            while Instant::now() < grace {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            return Err("the real convergence fixture did not stop after the CUA journey".into());
        }
        thread::sleep(Duration::from_millis(50));
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

fn state_string<'a>(state: &'a Value, field: &str) -> Result<&'a str, String> {
    state
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("the real fixture readiness document has no {field}"))
}

fn absent(tree: &str, needles: &[&str], why: &str) -> Result<(), String> {
    for needle in needles {
        if tree.contains(needle) {
            return Err(format!("{why}: the screen shows {needle:?}"));
        }
    }
    Ok(())
}

fn receipt_ready(tree: &str) -> bool {
    tree.contains("Convergence receipt")
        && Regex::new(r"exit [1-9][0-9]*").unwrap().is_match(tree)
        && tree.contains("skarbiec")
        && Regex::new(r#""status"\s*:\s*"failed""#)
            .unwrap()
            .is_match(tree)
}

fn open_services(
    driver: &crate::cua::Driver,
    app: &App,
    target: &str,
) -> Result<console::View, String> {
    console::click(driver, app.pid, app.window_id, "Services")?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let view = console::read_window(driver, app.pid, app.window_id)?;
        if view.tree.contains(target) && Regex::new(r"(?i)skarbiec").unwrap().is_match(&view.tree) {
            return Ok(view);
        }
        if Regex::new(r"Sign In|Continue with|Enter your email|Connect to Stado")
            .unwrap()
            .is_match(&view.tree)
        {
            return Err("The dedicated local registry API client unexpectedly requested an account sign-in; this journey performs no Wisent account operation or provider flow.".into());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Services never rendered the real host-wide convergence report; last tree: {}",
                super::common::tail(&view.tree, 2500)
            ));
        }
        if view.tree.contains("Retry") {
            console::click(driver, app.pid, app.window_id, "Retry")?;
        }
        thread::sleep(Duration::from_millis(500));
    }
}

pub fn run(context: &specs::Context) -> Result<(), String> {
    let source = console::require_product_dispatch(context)?;
    let built_cli = required(
        context,
        "PROBIERZ_STADO_BIN",
        "PROBIERZ_STADO_BIN must identify the CLI built from that staged source",
    )?;
    let built_cli = PathBuf::from(built_cli);
    if !built_cli.exists() {
        return Err(format!(
            "the staged-source Stado CLI is absent: {}",
            built_cli.display()
        ));
    }

    let crate_root = source.join("stado-rs");
    let control = context.artifacts.join("stado-service-convergence-fixture");
    fs::create_dir_all(&control).map_err(|error| error.to_string())?;
    let ready = control.join("ready.json");
    let stop = control.join("stop");
    let stdout_path = control.join("fixture.stdout.log");
    let stderr_path = control.join("fixture.stderr.log");
    let mut stdout_options = OpenOptions::new();
    stdout_options.create(true).truncate(true).write(true);
    let mut stderr_options = OpenOptions::new();
    stderr_options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        stdout_options.mode(0o600);
        stderr_options.mode(0o600);
    }
    let stdout = stdout_options
        .open(&stdout_path)
        .map_err(|error| error.to_string())?;
    let stderr = stderr_options
        .open(&stderr_path)
        .map_err(|error| error.to_string())?;
    let mut fixture = Command::new("cargo")
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
        .current_dir(&crate_root)
        .env("STADO_SERVICE_CONVERGENCE_READY", &ready)
        .env("STADO_SERVICE_CONVERGENCE_STOP", &stop)
        .env("CARGO_PROFILE_TEST_DEBUG", "0")
        .env("CARGO_INCREMENTAL", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| error.to_string())?;

    let driver = match super::common::driver(context) {
        Ok(driver) => driver,
        Err(error) => {
            let _ = fs::write(&stop, "stop\n");
            let cleanup = wait_for_exit(&mut fixture, Duration::from_secs(30));
            return cleanup.and(Err(error));
        }
    };
    let mut app: Option<App> = None;
    let journey = (|| {
        wait_for_file(&ready, &mut fixture, Duration::from_secs(180))?;
        let state: Value =
            serde_json::from_slice(&fs::read(&ready).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        for field in [
            "endpoint",
            "home",
            "storage",
            "config",
            "binary",
            "target",
            "token_file",
        ] {
            state_string(&state, field)?;
        }
        let endpoint = state_string(&state, "endpoint")?;
        let home = PathBuf::from(state_string(&state, "home")?);
        let binary =
            fs::canonicalize(state_string(&state, "binary")?).map_err(|error| error.to_string())?;
        let expected_binary = fs::canonicalize(&built_cli).map_err(|error| error.to_string())?;
        if binary != expected_binary {
            return Err(
                "the GUI fixture and Stado Desktop are not using the same staged-source CLI".into(),
            );
        }
        let token_file = PathBuf::from(state_string(&state, "token_file")?);
        #[cfg(unix)]
        if fs::metadata(&token_file)
            .map_err(|error| error.to_string())?
            .mode()
            & 0o777
            != 0o600
        {
            return Err(
                "the dedicated registry API token file is not owner read/write only".into(),
            );
        }
        if home.join(".stado/local-storage/registry.json").exists() {
            return Err(
                "the isolated app HOME unexpectedly exposes the fixture target to a CLI fallback"
                    .into(),
            );
        }
        let target = state_string(&state, "target")?;
        let path = format!(
            "{}:/usr/bin:/bin:/usr/sbin:/sbin",
            built_cli.parent().unwrap_or(Path::new("")).display()
        );
        let environment = BTreeMap::from([
            ("HOME".to_string(), home.to_string_lossy().into_owned()),
            (
                "CFFIXED_USER_HOME".to_string(),
                home.to_string_lossy().into_owned(),
            ),
            (
                "TMPDIR".to_string(),
                home.join("tmp").to_string_lossy().into_owned(),
            ),
            ("PATH".to_string(), path),
            ("STADO_REGISTRY_API_URL".to_string(), endpoint.to_string()),
            (
                "STADO_REGISTRY_API_TOKEN_FILE".to_string(),
                token_file.to_string_lossy().into_owned(),
            ),
        ]);
        let arguments = vec!["-dashboardBaseURL".to_string(), endpoint.to_string()];
        let launched = console::launch_console_with(context, &driver, &environment, &arguments)?;
        app = Some(launched.clone());

        let loaded = open_services(&driver, &launched, target)?;
        absent(
            &loaded.tree,
            &[
                "Connect to Stado",
                "This source cannot be read",
                "Sign In",
                "Continue with",
                "Enter your email",
            ],
            "the documented local control path selected an unreadable source or requested an account operation",
        )?;
        if !Regex::new(r"(?i)skarbiec").unwrap().is_match(&loaded.tree) {
            return Err("the real host-wide GET report is absent".into());
        }
        console::capture(
            context,
            &driver,
            launched.pid,
            launched.window_id,
            "stado-services-convergence",
            "host-wide-report-before-apply",
        )?;

        let (sheet_window, sheet, _) = console::activate(
            &driver,
            launched.pid,
            launched.window_id,
            "Converge…",
            "\"Converge declared service binaries\"",
            |tree| tree.contains("Converge declared service binaries"),
            Duration::from_secs(30),
        )?;
        if !sheet.tree.contains(target) {
            return Err(format!("the convergence sheet does not name {target}"));
        }
        for needle in ["All declared binaries", "stado service converge"] {
            if !sheet.tree.contains(needle) {
                return Err(format!("the convergence sheet does not show {needle}"));
            }
        }
        console::capture(
            context,
            &driver,
            launched.pid,
            sheet_window,
            "stado-services-convergence",
            "local-registry-client-host-wide-confirmation",
        )?;
        console::click(&driver, launched.pid, sheet_window, "Apply convergence")?;

        let receipt = console::wait_for_screen(
            &driver,
            launched.pid,
            launched.window_id,
            receipt_ready,
            "a state this journey reads",
            Duration::from_secs(180),
        )?;
        let target_pattern =
            Regex::new(&format!(r#""target"\s*:\s*"{}""#, regex::escape(target))).unwrap();
        if !target_pattern.is_match(&receipt.tree) {
            return Err(format!(
                "the convergence receipt does not name target {target}"
            ));
        }
        for (pattern, message) in [
            (
                r#""verdict"\s*:\s*"host-behind""#,
                "the receipt has no host-behind verdict",
            ),
            (
                r#""verdict"\s*:\s*"in-sync""#,
                "the receipt has no in-sync verdict",
            ),
        ] {
            if !Regex::new(pattern).unwrap().is_match(&receipt.tree) {
                return Err(message.to_string());
            }
        }
        console::capture(
            context,
            &driver,
            launched.pid,
            launched.window_id,
            "stado-services-convergence",
            "complete-failed-receipt",
        )?;
        console::click(&driver, launched.pid, launched.window_id, "Refresh")?;
        console::wait_for_screen(
            &driver,
            launched.pid,
            launched.window_id,
            receipt_ready,
            "a state this journey reads",
            Duration::from_secs(180),
        )?;
        console::capture(
            context,
            &driver,
            launched.pid,
            launched.window_id,
            "stado-services-convergence",
            "failed-receipt-retained-after-refresh",
        )?;
        Ok(())
    })();

    if journey.is_err() {
        if let Some(app) = &app {
            let _ = console::dump_windows(
                context,
                &driver,
                app.pid,
                "stado-services-convergence-failure",
            );
        }
    }
    if let Some(app) = &app {
        driver.quit_app(app.pid);
    }
    let stop_result = fs::write(&stop, "stop\n").map_err(|error| error.to_string());
    let exit_result = wait_for_exit(&mut fixture, Duration::from_secs(30));
    journey.and(stop_result).and(exit_result)
}
