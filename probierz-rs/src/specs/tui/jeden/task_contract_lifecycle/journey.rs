//! The run: the product's own contract tests, the settings on both
//! paths, then four real model turns — create, edit, delete a file,
//! and answer without calling a tool at all.

use super::*;

/// Every turn in this journey is an explicitly requested isolated
/// exercise, not product development, so the model is told not to
/// invent software, documentation, tests or commits for it.
const EXERCISE_PREFIX: &str = "This is an explicitly requested isolated file-tool exercise, not new product development. Do not create software, documentation, tests or commits for it. Explain inapplicable delivery requirements honestly in the final structured report. ";

/// The file the three file-tool turns create, change and remove.
const LIFECYCLE_FILE: &str = "lifecycle.txt";

/// Settings the real model turns need, which the run must supply.
const MODEL_SETTINGS: [&str; 5] = [
    "BRAMA_URL",
    "BRAMA_TOKEN",
    "WISENT_APP_AGENT_ID",
    "WISENT_APP_AGENT_AUTH_SECRET",
    "JEDEN_MODEL",
];

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = common::required(
        context,
        "TUI_CMD",
        "TUI_CMD must name the source-bound Jeden binary",
    )?;
    if !Path::new(&binary).is_absolute() {
        return Err("TUI_CMD must name the source-bound Jeden binary".into());
    }

    let root = isolated_root(context)?;
    let home = root.join("home");
    let workspace = root.join("workspace");
    let sessions = root.join("sessions");
    let temporary = root.join("temporary");
    for path in [&home, &workspace, &sessions, &temporary] {
        fs::create_dir_all(path).map_err(|e| e.to_string())?;
    }
    let mut env = common::env_map([
        ("HOME", home.to_string_lossy().as_ref()),
        ("JEDEN_SESSION_ROOT", sessions.to_string_lossy().as_ref()),
        ("TMPDIR", temporary.to_string_lossy().as_ref()),
        ("JEDEN_LANGUAGE", "en"),
    ]);

    let trace_path = root.join("trace.json");
    let result = (|| {
        run_contract_tests(context, &temporary)?;
        check_cli_settings(&binary, &workspace, &home, &env)?;
        check_rpc_settings(&binary, &workspace, &home, &env)?;

        for name in MODEL_SETTINGS {
            let value = common::required(
                context,
                name,
                &format!("{name} must be supplied by the real Brama workload configuration"),
            )?;
            env.insert(name.into(), value);
        }
        run_file_lifecycle(&binary, &workspace, &sessions, &env)?;
        run_without_tools(&binary, &workspace, &sessions, &env)?;

        write_trace(&trace_path, &workspace, &sessions, "completed",
            "Jeden task-contract lifecycle exited with status 0.")?;
        context.media_typed("trace", trace_path.clone(), "application/json");
        Ok(())
    })();

    if result.is_err() {
        let _ = write_trace(&trace_path, &workspace, &sessions, "failed",
            "The task-contract lifecycle has not completed.");
    }
    result
}

/// A root of this run's own, so nothing here touches an operator's
/// home, sessions or temporary files.
fn isolated_root(context: &specs::Context) -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(context
        .artifacts
        .join(format!("task-contract-{}-{stamp}", std::process::id())))
}

/// The product's own operator-contract tests, run from its source.
fn run_contract_tests(context: &specs::Context, temporary: &Path) -> Result<(), String> {
    let repository = context.harness.join("packages");
    let arguments = common::strings(&[
        "test",
        "--locked",
        "--release",
        "--test",
        "contracts",
        "operator_contracts_",
        "--",
        "--nocapture",
    ]);
    let tests = common::run(
        "cargo",
        &arguments,
        Some(&repository),
        &common::env_map([("TMPDIR", temporary.to_string_lossy().as_ref())]),
        &[],
        None,
        LONG_TIMEOUT,
    )?;
    if !tests.status.success() {
        return Err(format!(
            "cargo {}\n{}\n{}",
            arguments.join(" "),
            tests.stderr,
            tests.stdout
        ));
    }
    Ok(())
}

/// Create, change and remove one file through the real file tools,
/// checking the workspace after each turn.
fn run_file_lifecycle(
    binary: &str,
    workspace: &Path,
    sessions: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    for (task, expected) in [
        (
            format!("{EXERCISE_PREFIX}Create {LIFECYCLE_FILE} containing exactly alpha, using the real file tools, then read it back."),
            Some("alpha"),
        ),
        (
            format!("{EXERCISE_PREFIX}Edit {LIFECYCLE_FILE} so its entire content is exactly beta, using the real file tools, then read it back."),
            Some("beta"),
        ),
        (
            format!("{EXERCISE_PREFIX}Delete {LIFECYCLE_FILE} using the real file tools."),
            None,
        ),
    ] {
        model_turn(binary, &task, workspace, sessions, env)?;
        let file = workspace.join(LIFECYCLE_FILE);
        match expected {
            Some(content) => {
                let written = fs::read_to_string(&file).map_err(|e| e.to_string())?;
                if written != content {
                    return Err(format!(
                        "{LIFECYCLE_FILE} did not contain exactly {content}"
                    ));
                }
            }
            None => {
                if file.exists() {
                    return Err(format!("{LIFECYCLE_FILE} still exists after delete task"));
                }
            }
        }
    }
    Ok(())
}

/// A turn that must answer without calling a tool, and still produce
/// the structured report.
fn run_without_tools(
    binary: &str,
    workspace: &Path,
    sessions: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let events = model_turn(
        binary,
        "Answer 2 + 2 without calling any tool, and provide the required structured delivery report with honest explanations of inapplicable requirements.",
        workspace,
        sessions,
        env,
    )?;
    if events.iter().any(|e| e["type"] == "tool_call") {
        return Err("no-tools task called a tool".into());
    }
    Ok(())
}

fn write_trace(
    trace_path: &Path,
    workspace: &Path,
    sessions: &Path,
    status: &str,
    reply: &str,
) -> Result<(), String> {
    common::write_json(
        trace_path,
        &json!({
            "schemaVersion": TRACE_SCHEMA_VERSION,
            "kind": "probierz-jeden-task-contract-lifecycle",
            "status": status,
            "observation": {"reply": reply},
            "workspace": workspace,
            "sessions": sessions
        }),
    )
}
