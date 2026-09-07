use crate::specs::{self, tui::common};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
const REQUIREMENTS: [&str; 7] = [
    "functionality",
    "diagnostics",
    "cli",
    "gui",
    "documentation",
    "tests",
    "delivery",
];
fn check_contract(value: &Value) -> Result<(), String> {
    if value["version"] != 1 {
        return Err("task contract version must be 1".into());
    }
    let mut ids = value["requirements"]
        .as_array()
        .ok_or("task contract requirements must be an array")?
        .iter()
        .filter_map(|x| x["id"].as_str())
        .collect::<Vec<_>>();
    ids.sort();
    let mut required = REQUIREMENTS.to_vec();
    required.sort();
    if ids != required {
        return Err(format!("task contract requirements differ: {ids:?}"));
    }
    Ok(())
}
fn command(
    binary: &str,
    args: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    input: Option<&str>,
    timeout: Duration,
) -> Result<common::Output, String> {
    common::run(binary, args, Some(cwd), env, &[], input, timeout)
}
fn rpc(
    binary: &str,
    method: &str,
    params: Value,
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Value, String> {
    let input = format!(
        "{}\n{}\n",
        json!({"id":"contract","method":method,"params":params}),
        json!({"id":"shutdown","method":"shutdown","params":{}})
    );
    let result = command(
        binary,
        &["rpc".into()],
        cwd,
        env,
        Some(&input),
        Duration::from_secs(300),
    )?;
    if !result.status.success() {
        return Err(format!(
            "{} rpc\n{}\n{}",
            binary, result.stderr, result.stdout
        ));
    }
    result
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .find_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["id"] == "contract")
        .ok_or("RPC must return the requested response".into())
}
fn model_turn(
    binary: &str,
    task: &str,
    workspace: &Path,
    sessions: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Vec<Value>, String> {
    let result = command(
        binary,
        &vec![
            "run".into(),
            task.into(),
            "--json".into(),
            "--allow-write".into(),
            "--max-steps".into(),
            "24".into(),
        ],
        workspace,
        env,
        None,
        Duration::from_secs(900),
    )?;
    if !result.status.success() {
        return Err(format!(
            "{binary} run\n{}\n{}",
            result.stderr, result.stdout
        ));
    }
    let answer = common::parse_json(&result.stdout, "Jeden run")?;
    if answer["ok"] != true {
        return Err("Jeden run answer was not ok".into());
    }
    let session = PathBuf::from(
        answer["sessionPath"]
            .as_str()
            .ok_or("sessionPath missing")?,
    );
    if !session.starts_with(sessions) {
        return Err("the turn must use its isolated session root".into());
    }
    let transcript =
        fs::read_to_string(session.join("transcript.jsonl")).map_err(|e| e.to_string())?;
    let events = transcript
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str::<Value>(l).map(|e| e.get("payload").cloned().unwrap_or(e)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let contracts = events
        .iter()
        .filter(|e| e["type"] == "task_contract")
        .collect::<Vec<_>>();
    if contracts.len() != 1 {
        return Err("a task must retain exactly one task_contract".into());
    }
    check_contract(&contracts[0]["data"])?;
    if contracts[0]["data"]["task"] != task {
        return Err("retained task contract names a different task".into());
    }
    let reports = events
        .iter()
        .filter(|e| e["type"] == "task_report")
        .collect::<Vec<_>>();
    if reports.len() != 1 {
        return Err("a successful task must retain one complete delivery report".into());
    }
    let report = &reports[0]["data"];
    if report["status"] != "complete" {
        return Err("blocked work must not pass as completed".into());
    }
    for name in REQUIREMENTS {
        let entry = &report["report"][name];
        if entry["status"] != "done" && entry["status"] != "not_applicable" {
            return Err(format!("{name} has invalid report status"));
        }
        if !entry["explanation"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty())
        {
            return Err(format!("{name} has no explanation"));
        }
        if entry["status"] == "done"
            && !entry["evidence"].as_array().is_some_and(|a| {
                a.iter()
                    .any(|r| r.as_str().is_some_and(|s| !s.trim().is_empty()))
            })
        {
            return Err(format!("{name} done entry has no evidence"));
        }
    }
    let final_text = events
        .iter()
        .rev()
        .find(|e| e["type"] == "final")
        .and_then(|e| e["data"]["text"].as_str())
        .unwrap_or("")
        .trim();
    if final_text != answer["text"].as_str().unwrap_or("").trim() {
        return Err("final transcript and answer differ".into());
    }
    if events
        .iter()
        .any(|e| e["type"] == "contract_violation" && e["data"]["outcome"] == "rejected")
    {
        return Err("contract violation was rejected".into());
    }
    Ok(events)
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = common::required(
        context,
        "TUI_CMD",
        "TUI_CMD must name the source-bound Jeden binary",
    )?;
    if !Path::new(&binary).is_absolute() {
        return Err("TUI_CMD must name the source-bound Jeden binary".into());
    }
    let root = common::scratch("task-contract")?;
    let home = root.join("home");
    let workspace = root.join("workspace");
    let sessions = root.join("sessions");
    let temporary = root.join("temporary");
    for p in [&home, &workspace, &sessions, &temporary] {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let env = common::env_map([
        ("HOME", home.to_string_lossy().as_ref()),
        ("JEDEN_SESSION_ROOT", sessions.to_string_lossy().as_ref()),
        ("TMPDIR", temporary.to_string_lossy().as_ref()),
        ("JEDEN_LANGUAGE", "en"),
    ]);
    let trace_path = root.join("trace.json");
    let result = (|| {
        let repository = context.harness.join("packages");
        let tests = common::run(
            "cargo",
            &common::strings(&[
                "test",
                "--locked",
                "--release",
                "--test",
                "contracts",
                "operator_contracts_",
                "--",
                "--nocapture",
            ]),
            Some(&repository),
            &common::env_map([("TMPDIR", temporary.to_string_lossy().as_ref())]),
            &[],
            None,
            Duration::from_secs(900),
        )?;
        if !tests.status.success() {
            return Err(format!("cargo test --locked --release --test contracts operator_contracts_ -- --nocapture\n{}\n{}",tests.stderr,tests.stdout));
        }
        for value in ["Use plain sentences.", "Answer in Polish."] {
            let set = command(
                &binary,
                &common::strings(&["config", "set", "contracts.communication", value]),
                &workspace,
                &env,
                None,
                Duration::from_secs(300),
            )?;
            if !set.status.success() {
                return Err(set.combined());
            }
            let settings = common::read_json(&home.join(".jeden/config.yml"))?;
            if settings["contracts"]["communication"] != value {
                return Err("CLI contract communication did not persist".into());
            }
            let get = command(
                &binary,
                &common::strings(&["config", "get", "contracts.communication"]),
                &workspace,
                &env,
                None,
                Duration::from_secs(300),
            )?;
            if get.stdout.trim() != value {
                return Err("CLI config get returned a different communication value".into());
            }
        }
        command(
            &binary,
            &common::strings(&[
                "config",
                "set",
                "contracts.functionality",
                "Complete the requested operation.",
            ]),
            &workspace,
            &env,
            None,
            Duration::from_secs(300),
        )?;
        command(
            &binary,
            &common::strings(&["config", "reset", "contracts.functionality"]),
            &workspace,
            &env,
            None,
            Duration::from_secs(300),
        )?;
        let before = fs::read(home.join(".jeden/config.yml")).map_err(|e| e.to_string())?;
        let refused = command(
            &binary,
            &common::strings(&["config", "get", "contracts.style"]),
            &workspace,
            &env,
            None,
            Duration::from_secs(300),
        )?;
        if refused.code() != Some(1)
            || refused.stderr.trim() != "Error: unknown config key: contracts.style"
            || fs::read(home.join(".jeden/config.yml")).map_err(|e| e.to_string())? != before
        {
            return Err(
                "unknown config key refusal changed settings or answered incorrectly".into(),
            );
        }
        let initial = rpc(&binary, "config/contracts/get", json!({}), &workspace, &env)?;
        if !initial["error"].is_null() {
            return Err("config/contracts/get returned an error".into());
        }
        check_contract(&initial["result"]["taskContract"])?;
        let saved = rpc(
            &binary,
            "config/contracts/set",
            json!({"communication":"Be concise.","functionality":"Finish the task."}),
            &workspace,
            &env,
        )?;
        check_contract(&saved["result"]["taskContract"])?;
        let before = fs::read(home.join(".jeden/config.yml")).map_err(|e| e.to_string())?;
        let refused = rpc(
            &binary,
            "config/contracts/set",
            json!({"communication":"Incomplete request."}),
            &workspace,
            &env,
        )?;
        if refused["error"]["code"] != "invalid_params"
            || refused["error"]["message"] != "functionality must be a string"
            || fs::read(home.join(".jeden/config.yml")).map_err(|e| e.to_string())? != before
        {
            return Err("invalid RPC settings were not refused atomically".into());
        }
        for name in [
            "BRAMA_URL",
            "BRAMA_TOKEN",
            "WISENT_APP_AGENT_ID",
            "WISENT_APP_AGENT_AUTH_SECRET",
            "JEDEN_MODEL",
        ] {
            common::required(
                context,
                name,
                &format!("{name} must be supplied by the real Brama workload configuration"),
            )?;
        }
        let prefix="This is an explicitly requested isolated file-tool exercise, not new product development. Do not create software, documentation, tests or commits for it. Explain inapplicable delivery requirements honestly in the final structured report. ";
        model_turn(&binary,&format!("{prefix}Create lifecycle.txt containing exactly alpha, using the real file tools, then read it back."),&workspace,&sessions,&env)?;
        if fs::read_to_string(workspace.join("lifecycle.txt")).map_err(|e| e.to_string())?
            != "alpha"
        {
            return Err("created lifecycle.txt did not contain exactly alpha".into());
        }
        model_turn(&binary,&format!("{prefix}Edit lifecycle.txt so its entire content is exactly beta, using the real file tools, then read it back."),&workspace,&sessions,&env)?;
        if fs::read_to_string(workspace.join("lifecycle.txt")).map_err(|e| e.to_string())? != "beta"
        {
            return Err("edited lifecycle.txt did not contain exactly beta".into());
        }
        model_turn(
            &binary,
            &format!("{prefix}Delete lifecycle.txt using the real file tools."),
            &workspace,
            &sessions,
            &env,
        )?;
        if workspace.join("lifecycle.txt").exists() {
            return Err("lifecycle.txt still exists after delete task".into());
        }
        let no_tools=model_turn(&binary,"Answer 2 + 2 without calling any tool, and provide the required structured delivery report with honest explanations of inapplicable requirements.",&workspace,&sessions,&env)?;
        if no_tools.iter().any(|e| e["type"] == "tool_call") {
            return Err("no-tools task called a tool".into());
        }
        common::write_json(
            &trace_path,
            &json!({"schemaVersion":1,"kind":"probierz-jeden-task-contract-lifecycle","status":"completed","observation":{"reply":"Jeden task-contract lifecycle exited with status 0."},"workspace":workspace,"sessions":sessions}),
        )?;
        context.media_typed("trace", trace_path.clone(), "application/json");
        Ok(())
    })();
    if result.is_err() {
        let _ = common::write_json(
            &trace_path,
            &json!({"schemaVersion":1,"kind":"probierz-jeden-task-contract-lifecycle","status":"failed","observation":{"reply":"The task-contract lifecycle has not completed."},"workspace":workspace,"sessions":sessions}),
        );
    }
    result
}
