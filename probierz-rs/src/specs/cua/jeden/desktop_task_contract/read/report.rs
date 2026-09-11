use super::super::*;
use serde_json::json;
pub(crate) fn wait_for_session(
    driver: &Driver,
    app: &App,
    session_id: &str,
    trace: &mut Vec<Value>,
    last_tree: &mut Option<String>,
) -> Result<View, String> {
    let identifier = format!("id=session-row-{session_id}");
    let deadline = Instant::now() + SHELL_TIMEOUT;
    let mut last = None;
    while Instant::now() < deadline {
        let current = observe(
            driver,
            app.pid,
            app.window_id,
            "wait:isolated-session",
            trace,
            last_tree,
            None,
            false,
        )?;
        authorized(&current.tree)?;
        if current.tree.contains(&identifier) {
            return Ok(current);
        }
        last = Some(current);
        thread::sleep(POLL);
    }
    Err(format!("Jeden Desktop did not expose the isolated real session {session_id}; last accessibility tree: {}", common::tail(&last.map(|view| view.tree).unwrap_or_default(), 2000)))
}

pub(crate) fn wait_for_report(
    driver: &Driver,
    app: &App,
    recorded: &Recorded,
    trace: &mut Vec<Value>,
    last_tree: &mut Option<String>,
) -> Result<View, String> {
    let expected = normalize(&recorded.final_text);
    let deadline = Instant::now() + CONTRACT_TIMEOUT;
    let mut last = None;
    while Instant::now() < deadline {
        let current = observe(
            driver,
            app.pid,
            app.window_id,
            "wait:rendered-task-report",
            trace,
            last_tree,
            None,
            false,
        )?;
        authorized(&current.tree)?;
        let values = common::static_texts(&current.tree)
            .into_iter()
            .map(|value| normalize(&value))
            .collect::<Vec<_>>();
        if current.tree.contains("id=conversation-entry-final") && values.contains(&expected) {
            return Ok(current);
        }
        last = Some(current);
        thread::sleep(POLL);
    }
    Err(format!("Jeden Desktop did not render the real recorded final answer within 60000 ms; last accessibility tree: {}", common::tail(&last.map(|view| view.tree).unwrap_or_default(), 3000)))
}

pub(crate) fn publish_trace(context: &specs::Context, trace: &[Value]) -> Result<(), String> {
    fs::create_dir_all(&context.artifacts).map_err(|error| error.to_string())?;
    let file = context
        .artifacts
        .join(format!("{}-accessibility-trace.json", context.title));
    fs::write(
        &file,
        format!(
            "{}\n",
            serde_json::to_string_pretty(trace).unwrap_or_default()
        ),
    )
    .map_err(|error| format!("{}: {error}", file.display()))
}

pub fn run(context: &specs::Context) -> Result<(), String> {
    let job_id = require_remote(context)?;
    let executable = required_path(context, "CUA_APP_EXECUTABLE", true)?;
    let home = std::env::var("HOME")
        .map_err(|_| "HOME is required for the Jeden Desktop task-contract journey".to_string())?;
    let state_root = context
        .artifacts
        .join(format!("{}-{job_id}", context.title));
    let sessions_root = state_root.join("sessions");
    let workspace_name = format!("probierz-{job_id}");
    let workspace_root = PathBuf::from(home)
        .join("Documents/CodingProjects/Wisent")
        .join(&workspace_name);
    let task_marker = format!("Native report {job_id}");
    let task = format!("Answer with exactly \"{task_marker}\" before the required task delivery report. This is a read-only question, not an implementation request. Do not call tools or change files, settings, or services. Explain honestly why each inapplicable delivery requirement does not apply; nothing is blocked.");
    let mut trace = Vec::new();
    let mut last_tree = None;
    let mut app: Option<App> = None;
    let driver = common::driver(context)?;
    let result = (|| {
        let backend = record_backend(
            context,
            &workspace_name,
            &workspace_root,
            &sessions_root,
            &task,
            &mut trace,
        )?;
        let recorded = inspect_recorded(
            &backend,
            &sessions_root,
            &workspace_root,
            &task,
            &task_marker,
            &mut trace,
        )?;
        let environment = BTreeMap::from([
            ("JEDEN_LANGUAGE".to_string(), "en".to_string()),
            (
                "JEDEN_SESSION_ROOT".to_string(),
                sessions_root.to_string_lossy().into_owned(),
            ),
        ]);
        let launched = driver.launch_process(&executable, &environment, &[])?;
        app = Some(launched.clone());
        wait_for_shell(&driver, &launched, &mut trace, &mut last_tree)?;
        click_fresh(
            &driver,
            &launched,
            "AXButton (Settings)",
            "open Settings",
            &mut trace,
            &mut last_tree,
        )?;
        wait_for_contract(&driver, &launched, &mut trace, &mut last_tree)?;
        let loaded = capture(
            context,
            &driver,
            launched.pid,
            launched.window_id,
            "task-contract-loaded",
            &mut trace,
            &mut last_tree,
        )?;
        assert_contract_visible(&loaded.tree, &backend.contract)?;
        wait_for_session(
            &driver,
            &launched,
            &recorded.session_id,
            &mut trace,
            &mut last_tree,
        )?;
        click_fresh(
            &driver,
            &launched,
            &format!("id=session-row-{}", recorded.session_id),
            "open isolated report session",
            &mut trace,
            &mut last_tree,
        )?;
        wait_for_report(&driver, &launched, &recorded, &mut trace, &mut last_tree)?;
        let rendered = capture(
            context,
            &driver,
            launched.pid,
            launched.window_id,
            "task-report-rendered",
            &mut trace,
            &mut last_tree,
        )?;
        assert_report_visible(&rendered.tree, &backend.contract, &recorded)
    })();

    let cleanup_result = if let Some(app) = &app {
        (|| {
            observe(
                &driver,
                app.pid,
                app.window_id,
                "before-closing-candidate",
                &mut trace,
                &mut last_tree,
                None,
                true,
            )?;
            driver.hotkey(app.pid, &["cmd", "q"])?;
            let closed = driver.call(
                "verify_state",
                json!({
                    "pid": app.pid,
                    "window_id": app.window_id,
                    "expect": [{"window":{"exists":false}}],
                    "timeout_ms": 10_000,
                    "include_screenshot": false
                }),
            )?;
            trace.push(json!({"label":"candidate-window-closed","result":closed}));
            if closed.get("status").and_then(Value::as_str) != Some("satisfied") {
                return Err(
                    "The isolated candidate window must close after the journey".to_string()
                );
            }
            Ok(())
        })()
    } else {
        Ok(())
    };
    let remove_result = fs::remove_dir_all(&workspace_root)
        .or_else(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(|error| error.to_string());
    let publish_result = publish_trace(context, &trace);
    result
        .and(cleanup_result)
        .and(remove_result)
        .and(publish_result)
}
