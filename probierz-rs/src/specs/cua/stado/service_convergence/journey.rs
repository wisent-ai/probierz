//! The journey: launch Stado Desktop against the real fixture, read
//! the host-wide convergence report, apply convergence through the
//! application's own sheet, and read the receipt it leaves behind.

use super::*;

/// How long the sheet may take to appear after its button is pressed.
const SHEET_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a convergence receipt may take to render.
const RECEIPT_TIMEOUT: Duration = Duration::from_secs(180);

/// Permissions the fixture's dedicated API token file must carry.
#[cfg(unix)]
const TOKEN_MODE: u32 = 0o600;

/// Fields the readiness document must carry before the journey starts.
const READY_FIELDS: [&str; 7] = [
    "endpoint",
    "home",
    "storage",
    "config",
    "binary",
    "target",
    "token_file",
];

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

    let (mut fixture, control) = spawn(context, &source.join("stado-rs"))?;

    let driver = match crate::specs::cua::common::driver(context) {
        Ok(driver) => driver,
        Err(error) => {
            let _ = fs::write(&control.stop, "stop\n");
            let cleanup = wait_for_exit(&mut fixture, STOP_TIMEOUT);
            return cleanup.and(Err(error));
        }
    };
    let mut app: Option<App> = None;
    let journey = (|| {
        wait_for_file(&control.ready, &mut fixture, READY_TIMEOUT)?;
        let state = readiness(&control.ready)?;
        let target = state_string(&state, "target")?;
        let environment = client_environment(&state, &built_cli)?;

        let arguments = vec![
            "-dashboardBaseURL".to_string(),
            state_string(&state, "endpoint")?.to_string(),
        ];
        let launched = console::launch_console_with(context, &driver, &environment, &arguments)?;
        app = Some(launched.clone());

        let loaded = open_services(&driver, &launched, target)?;
        absent(
            &loaded.tree,
            &OFF_PATH_SCREENS,
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

        apply_convergence(context, &driver, &launched, target)?;
        read_receipt(context, &driver, &launched, target)?;
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
    let stop_result = fs::write(&control.stop, "stop\n").map_err(|error| error.to_string());
    let exit_result = wait_for_exit(&mut fixture, STOP_TIMEOUT);
    journey.and(stop_result).and(exit_result)
}

/// Read the fixture's readiness document and check every field the
/// journey depends on is present.
fn readiness(ready: &Path) -> Result<Value, String> {
    let state: Value = serde_json::from_slice(&fs::read(ready).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    for field in READY_FIELDS {
        state_string(&state, field)?;
    }
    Ok(state)
}

/// The environment Stado Desktop is launched with: the fixture's
/// isolated HOME, its dedicated registry endpoint and token file, and a
/// PATH whose Stado CLI is the one built from the staged source.
///
/// Refuses when the desktop and the fixture would use different CLIs,
/// when the token file is readable by anyone else, or when the isolated
/// HOME exposes local storage the CLI could read instead of the
/// fixture's registry.
fn client_environment(
    state: &Value,
    built_cli: &Path,
) -> Result<BTreeMap<String, String>, String> {
    let endpoint = state_string(state, "endpoint")?;
    let home = PathBuf::from(state_string(state, "home")?);
    let binary =
        fs::canonicalize(state_string(state, "binary")?).map_err(|error| error.to_string())?;
    let expected_binary = fs::canonicalize(built_cli).map_err(|error| error.to_string())?;
    if binary != expected_binary {
        return Err(
            "the GUI fixture and Stado Desktop are not using the same staged-source CLI".into(),
        );
    }
    let token_file = PathBuf::from(state_string(state, "token_file")?);
    #[cfg(unix)]
    if fs::metadata(&token_file)
        .map_err(|error| error.to_string())?
        .mode()
        & 0o777
        != TOKEN_MODE
    {
        return Err("the dedicated registry API token file is not owner read/write only".into());
    }
    if home.join(".stado/local-storage/registry.json").exists() {
        return Err(
            "the isolated app HOME unexpectedly exposes the fixture target to local CLI storage"
                .into(),
        );
    }

    let path = format!(
        "{}:/usr/bin:/bin:/usr/sbin:/sbin",
        built_cli.parent().unwrap_or(Path::new("")).display()
    );
    Ok(BTreeMap::from([
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
    ]))
}

/// Open the convergence sheet, check it names the target and the
/// command it will run, and apply it.
fn apply_convergence(
    context: &specs::Context,
    driver: &crate::cua::Driver,
    app: &App,
    target: &str,
) -> Result<(), String> {
    let (sheet_window, sheet, _) = console::activate(
        driver,
        app.pid,
        app.window_id,
        "Converge…",
        "\"Converge declared service binaries\"",
        |tree| tree.contains("Converge declared service binaries"),
        SHEET_TIMEOUT,
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
        driver,
        app.pid,
        sheet_window,
        "stado-services-convergence",
        "local-registry-client-host-wide-confirmation",
    )?;
    console::click(driver, app.pid, sheet_window, "Apply convergence").map(|_| ())
}

/// Read the receipt convergence left behind: it must name the target
/// and carry both verdicts, and it must survive a refresh.
fn read_receipt(
    context: &specs::Context,
    driver: &crate::cua::Driver,
    app: &App,
    target: &str,
) -> Result<(), String> {
    let receipt = console::wait_for_screen(
        driver,
        app.pid,
        app.window_id,
        receipt_ready,
        "a state this journey reads",
        RECEIPT_TIMEOUT,
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
        driver,
        app.pid,
        app.window_id,
        "stado-services-convergence",
        "complete-failed-receipt",
    )?;
    console::click(driver, app.pid, app.window_id, "Refresh")?;
    console::wait_for_screen(
        driver,
        app.pid,
        app.window_id,
        receipt_ready,
        "a state this journey reads",
        RECEIPT_TIMEOUT,
    )?;
    console::capture(
        context,
        driver,
        app.pid,
        app.window_id,
        "stado-services-convergence",
        "failed-receipt-retained-after-refresh",
    )
    .map(|_| ())
}
