use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::{json, Value};

use crate::{
    cua::{self, App, Driver, Snapshot},
    specs,
};

use crate::specs::cua::common;

const POLL: Duration = Duration::from_millis(500);
const SHELL_TIMEOUT: Duration = Duration::from_secs(45);
const CONTRACT_TIMEOUT: Duration = Duration::from_secs(60);
const REPORT_TIMEOUT: Duration = Duration::from_secs(150);
const DEDICATED_HOST: &str = "charless-mac-mini";
const REQUIRED_REPORT_ENTRIES: [&str; 7] = [
    "functionality",
    "diagnostics",
    "cli",
    "gui",
    "documentation",
    "tests",
    "delivery",
];

#[derive(Clone)]
struct View {
    tree: String,
    snapshot_id: Option<String>,
    elements: Vec<Value>,
    snapshot: Snapshot,
}

struct Backend {
    contract: Value,
    session_path: PathBuf,
}

struct Recorded {
    session_id: String,
    report: Value,
    final_text: String,
}

fn required(context: &specs::Context, name: &str) -> Result<String, String> {
    context
        .optional(name)
        .ok_or_else(|| format!("{name} is required for the Jeden Desktop task-contract journey"))
}

fn required_path(context: &specs::Context, name: &str, file: bool) -> Result<PathBuf, String> {
    let value = required(context, name)?;
    let path = PathBuf::from(&value);
    if !path.is_absolute() {
        return Err(format!(
            "{name} must be an absolute path, received {value:?}"
        ));
    }
    if !path.exists() {
        return Err(format!("{name} does not exist: {}", path.display()));
    }
    if file && !path.is_file() {
        return Err(format!("{name} is not a file: {}", path.display()));
    }
    Ok(path)
}

fn require_remote(context: &specs::Context) -> Result<String, String> {
    let job_id = required(context, "WC_JOB_ID")?;
    if !Regex::new(r"^job-[0-9a-f]{24}$").unwrap().is_match(&job_id) {
        return Err("This journey refuses local execution: WC_JOB_ID must be the canonical ID supplied by a real Stado worker job".to_string());
    }
    if std::env::consts::OS != "macos" {
        return Err(
            "The Jeden Desktop task-contract journey requires the Stado-selected macOS GUI worker"
                .to_string(),
        );
    }
    let hostname = Command::new("hostname")
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_default();
    let hostname = hostname.strip_suffix(".local").unwrap_or(&hostname);
    if hostname != DEDICATED_HOST {
        return Err("This journey may run on the dedicated Mac mini, never on the operator's current computer".to_string());
    }
    if required(context, "PROBIERZ_APP_ID")? != "jeden-desktop" {
        return Err(
            "The native task-contract journey may only run for the registered jeden-desktop app"
                .to_string(),
        );
    }
    let journeys: HashSet<String> = required(context, "PROBIERZ_JOURNEYS")?
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    if !journeys.contains("task-contract") {
        return Err(
            "The registered task-contract journey must authorize this product-owned spec"
                .to_string(),
        );
    }
    required_path(context, "PROBIERZ_SOURCE_IDENTITY", true)?;
    required_path(context, "PROBIERZ_APP_SOURCE", false)?;
    Ok(job_id)
}

fn view(snapshot: Snapshot) -> View {
    View {
        tree: snapshot.tree.clone(),
        snapshot_id: snapshot.snapshot_id.clone(),
        elements: snapshot.elements.clone(),
        snapshot,
    }
}

fn observed_at() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    millis.to_string()
}

fn observe(
    driver: &Driver,
    pid: u32,
    window_id: u64,
    label: &str,
    trace: &mut Vec<Value>,
    last_tree: &mut Option<String>,
    screenshot: Option<&Path>,
    force: bool,
) -> Result<View, String> {
    let snapshot = driver.snapshot_to(pid, window_id, screenshot)?;
    let view = view(snapshot);
    if force || last_tree.as_deref() != Some(&view.tree) {
        trace.push(json!({
            "observedAt": observed_at(),
            "label": label,
            "snapshotID": view.snapshot_id,
            "tree": view.tree,
            "elements": view.elements,
        }));
        *last_tree = Some(view.tree.clone());
    }
    Ok(view)
}

fn capture(
    context: &specs::Context,
    driver: &Driver,
    pid: u32,
    window_id: u64,
    name: &str,
    trace: &mut Vec<Value>,
    last_tree: &mut Option<String>,
) -> Result<View, String> {
    let file = context
        .artifacts
        .join(format!("{}-{name}.png", context.title));
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let view = observe(
        driver,
        pid,
        window_id,
        &format!("capture:{name}"),
        trace,
        last_tree,
        Some(&file),
        true,
    )?;
    let metadata = fs::metadata(&file)
        .map_err(|_| format!("cua-driver wrote no screenshot at {}", file.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!(
            "cua-driver wrote no screenshot at {}",
            file.display()
        ));
    }
    context.media("screenshot", file);
    Ok(view)
}

fn authorized(tree: &str) -> Result<(), String> {
    let visible = tree.contains("id=wisent.auth.screen")
        || tree.contains("Sign in with your Wisent account")
        || tree.contains("AXButton (Continue with GitHub)");
    if visible {
        Err("The Stado GUI worker has no authorized Wisent identity for Jeden Desktop; this journey refuses to request credentials or trigger consent UI".to_string())
    } else {
        Ok(())
    }
}

fn wait_for_shell(
    driver: &Driver,
    app: &App,
    trace: &mut Vec<Value>,
    last_tree: &mut Option<String>,
) -> Result<View, String> {
    let deadline = Instant::now() + SHELL_TIMEOUT;
    let mut last = None;
    while Instant::now() < deadline {
        let current = observe(
            driver,
            app.pid,
            app.window_id,
            "wait:authorized-shell",
            trace,
            last_tree,
            None,
            false,
        )?;
        authorized(&current.tree)?;
        if current.tree.contains("AXButton (Settings)") {
            return Ok(current);
        }
        last = Some(current);
        thread::sleep(POLL);
    }
    Err(format!(
        "Jeden Desktop did not expose its authorized Settings navigation within 45000 ms; last accessibility tree: {}",
        common::tail(&last.map(|view| view.tree).unwrap_or_default(), 1500)
    ))
}

fn click_fresh(
    driver: &Driver,
    app: &App,
    needle: &str,
    label: &str,
    trace: &mut Vec<Value>,
    last_tree: &mut Option<String>,
) -> Result<(), String> {
    let before = observe(
        driver,
        app.pid,
        app.window_id,
        &format!("before-action:{label}"),
        trace,
        last_tree,
        None,
        true,
    )?;
    authorized(&before.tree)?;
    let index = cua::element_index_of(&before.tree, needle)?;
    let element = before
        .elements
        .iter()
        .find(|element| element.get("element_index").and_then(Value::as_u64) == Some(index))
        .ok_or_else(|| format!("no indexed element matching {needle:?} in tree"))?;
    let result = driver.click_element(app.pid, app.window_id, &before.snapshot, element)?;
    if result.get("status").and_then(Value::as_str) == Some("refused") {
        return Err(format!("cua-driver refused the {label} action: {result}"));
    }
    Ok(())
}

fn wait_for_contract(
    driver: &Driver,
    app: &App,
    trace: &mut Vec<Value>,
    last_tree: &mut Option<String>,
) -> Result<View, String> {
    let deadline = Instant::now() + CONTRACT_TIMEOUT;
    let mut last = None;
    while Instant::now() < deadline {
        let current = observe(
            driver,
            app.pid,
            app.window_id,
            "wait:task-contract",
            trace,
            last_tree,
            None,
            false,
        )?;
        authorized(&current.tree)?;
        if current.tree.contains("id=task-contract-instructions")
            && current.tree.contains("id=task-contract-requirements")
        {
            return Ok(current);
        }
        let unavailable = current.tree.contains("id=task-contract-unavailable");
        let loading = current
            .tree
            .contains("The built-in task contract has not loaded yet.")
            || current.tree.contains("Loading contracts from Jeden");
        if unavailable && !loading {
            return Err(format!("Jeden Desktop reported that the real config/contracts/get RPC contract was unavailable: {}", common::tail(&current.tree, 2000)));
        }
        last = Some(current);
        thread::sleep(POLL);
    }
    Err(format!(
        "Jeden Desktop did not render the task contract returned by config/contracts/get within 60000 ms; last accessibility tree: {}",
        common::tail(&last.map(|view| view.tree).unwrap_or_default(), 2000)
    ))
}

fn write_frame(
    stdin: &mut impl Write,
    frame: &Value,
    requests: &mut Vec<Value>,
) -> Result<(), String> {
    requests.push(frame.clone());
    writeln!(stdin, "{frame}").map_err(|error| error.to_string())?;
    stdin.flush().map_err(|error| error.to_string())
}

fn sorted_requirement_ids(contract: &Value) -> Vec<String> {
    let mut ids: Vec<String> = contract
        .get("requirements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|requirement| {
            requirement
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    ids.sort();
    ids
}

fn required_ids() -> Vec<String> {
    let mut ids: Vec<String> = REQUIRED_REPORT_ENTRIES
        .iter()
        .map(|value| value.to_string())
        .collect();
    ids.sort();
    ids
}

fn record_backend(
    context: &specs::Context,
    workspace_name: &str,
    workspace_root: &Path,
    sessions_root: &Path,
    task: &str,
    trace: &mut Vec<Value>,
) -> Result<Backend, String> {
    fs::create_dir_all(sessions_root)
        .map_err(|error| format!("{}: {error}", sessions_root.display()))?;
    if workspace_root.exists() {
        return Err(format!(
            "The job-owned workspace already exists and cannot be treated as isolated: {}",
            workspace_root.display()
        ));
    }
    fs::create_dir(workspace_root)
        .map_err(|error| format!("{}: {error}", workspace_root.display()))?;
    let home = std::env::var("HOME")
        .map_err(|_| "HOME is required for the Jeden Desktop task-contract journey".to_string())?;
    let command = PathBuf::from(home).join(".stado/bin/stado");
    let args = [
        "host",
        "jeden-connect",
        workspace_name,
        "--target",
        DEDICATED_HOST,
    ];
    let stderr = Arc::new(Mutex::new(String::new()));
    let mut launch = Command::new(&command);
    launch
        .args(args)
        .current_dir(workspace_root)
        .env("JEDEN_LANGUAGE", "en")
        .env("JEDEN_SESSION_ROOT", sessions_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in [
        "STADO_MODEL_ROUTER_TOKEN",
        "PROBIERZ_MODEL_AGENT_SECRET",
        "PROBIERZ_SOURCE_IDENTITY",
        "PROBIERZ_APP_SOURCE",
        "WC_JOB_ID",
    ] {
        if let Some(value) = context.optional(name) {
            launch.env(name, value);
        }
    }
    let mut child = launch
        .spawn()
        .map_err(|error| format!("{}: {error}", command.display()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "The real Stado/Jeden connection has no stdout".to_string())?;
    let child_stderr = child
        .stderr
        .take()
        .ok_or_else(|| "The real Stado/Jeden connection has no stderr".to_string())?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "The real Stado/Jeden connection has no stdin".to_string())?;
    let (sender, receiver) = mpsc::channel::<Result<String, String>>();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender
                .send(line.map_err(|error| error.to_string()))
                .is_err()
            {
                break;
            }
        }
    });
    let stderr_sink = Arc::clone(&stderr);
    thread::spawn(move || {
        let mut reader = BufReader::new(child_stderr);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            stderr_sink.lock().expect("stderr lock").push_str(&line);
            line.clear();
        }
    });

    let deadline = Instant::now() + REPORT_TIMEOUT;
    let mut contract = None;
    let mut session_path = None;
    let mut rpc_session = None;
    let mut prompt_completed = false;
    let mut shutdown_acknowledged = false;
    let mut requests = Vec::new();
    let mut frames = Vec::new();
    let outcome = (|| {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("The real Stado/Jeden task did not finish within 150000 ms".to_string());
            }
            match receiver.recv_timeout(remaining.min(Duration::from_millis(500))) {
                Ok(Ok(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let frame: Value = serde_json::from_str(line.trim()).map_err(|error| {
                        format!("Jeden returned a non-JSON frame: {error}: {line}")
                    })?;
                    frames.push(frame.clone());
                    let method = frame.get("method").and_then(Value::as_str);
                    if matches!(
                        method,
                        Some("session/request_permission" | "session/request_input")
                    ) {
                        return Err(format!(
                            "The read-only report task unexpectedly requested interaction: {frame}"
                        ));
                    }
                    if frame.get("type").and_then(Value::as_str) == Some("ready")
                        && contract.is_none()
                    {
                        write_frame(
                            &mut stdin,
                            &json!({"id":"contract-oracle","method":"config/contracts/get","params":{}}),
                            &mut requests,
                        )?;
                    } else if frame.get("id").and_then(Value::as_str) == Some("contract-oracle") {
                        if !frame.get("error").unwrap_or(&Value::Null).is_null() {
                            return Err(format!("Jeden did not return the task contract: {frame}"));
                        }
                        let received = frame
                            .pointer("/result/taskContract")
                            .cloned()
                            .unwrap_or(Value::Null);
                        if received.get("version").and_then(Value::as_u64) != Some(1) {
                            return Err(
                                "The real backend must expose task contract version 1".to_string()
                            );
                        }
                        if sorted_requirement_ids(&received) != required_ids() {
                            return Err("The real backend task contract must expose the seven required report entries".to_string());
                        }
                        contract = Some(received);
                        write_frame(
                            &mut stdin,
                            &json!({"id":"session-open","method":"session/new","params":{"options":{"allowWrite":false,"allowCommand":false,"autoApprove":false,"maxSteps":24}}}),
                            &mut requests,
                        )?;
                    } else if frame.get("id").and_then(Value::as_str) == Some("session-open") {
                        if !frame.get("error").unwrap_or(&Value::Null).is_null() {
                            return Err(format!(
                                "Jeden could not create the isolated session: {frame}"
                            ));
                        }
                        rpc_session = frame
                            .pointer("/result/sessionId")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        session_path = frame
                            .pointer("/result/sessionPath")
                            .and_then(Value::as_str)
                            .map(PathBuf::from);
                        if rpc_session.is_none() || session_path.is_none() {
                            return Err(
                                "Jeden returned an incomplete session/new response".to_string()
                            );
                        }
                        write_frame(
                            &mut stdin,
                            &json!({"id":"report-turn","method":"session/prompt","params":{"sessionId":rpc_session,"requestId":"task-contract-native-report","prompt":task}}),
                            &mut requests,
                        )?;
                    } else if frame.get("id").and_then(Value::as_str) == Some("report-turn") {
                        if !frame.get("error").unwrap_or(&Value::Null).is_null() {
                            return Err(format!(
                                "The real task failed: {}",
                                frame.get("error").unwrap()
                            ));
                        }
                        prompt_completed = true;
                        write_frame(
                            &mut stdin,
                            &json!({"id":"shutdown","method":"shutdown","params":{}}),
                            &mut requests,
                        )?;
                    } else if frame.get("id").and_then(Value::as_str) == Some("shutdown") {
                        if !frame.get("error").unwrap_or(&Value::Null).is_null() {
                            return Err(format!("Jeden refused shutdown: {frame}"));
                        }
                        shutdown_acknowledged = true;
                        break;
                    }
                }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if child
                        .try_wait()
                        .map_err(|error| error.to_string())?
                        .is_some()
                    {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        drop(stdin);
        let remaining = deadline.saturating_duration_since(Instant::now());
        let status = wait_child(&mut child, remaining)?;
        if !status.success() {
            return Err(format!(
                "The real Stado/Jeden connection exited unsuccessfully: {}",
                stderr.lock().expect("stderr lock")
            ));
        }
        if !prompt_completed {
            return Err("The real session/prompt response was not received".to_string());
        }
        if !shutdown_acknowledged {
            return Err("The real Jeden RPC did not acknowledge shutdown".to_string());
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        let _ = child.kill();
        trace.push(json!({"label":"real-backend-task","command":command,"args":args,"workspaceRoot":workspace_root,"sessionsRoot":sessions_root,"requests":requests,"frames":frames,"stderr":*stderr.lock().expect("stderr lock"),"error":error}));
        return Err(error);
    }
    trace.push(json!({"label":"real-backend-task","command":command,"args":args,"workspaceRoot":workspace_root,"sessionsRoot":sessions_root,"requests":requests,"frames":frames,"stderr":*stderr.lock().expect("stderr lock"),"status":0}));
    Ok(Backend {
        contract: contract.unwrap_or(Value::Null),
        session_path: session_path.unwrap_or_default(),
    })
}

fn wait_child(child: &mut Child, timeout: Duration) -> Result<std::process::ExitStatus, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Err("The real Stado/Jeden task did not finish within 150000 ms".to_string());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn inspect_recorded(
    backend: &Backend,
    sessions_root: &Path,
    workspace_root: &Path,
    task: &str,
    task_marker: &str,
    trace: &mut Vec<Value>,
) -> Result<Recorded, String> {
    if !backend.session_path.starts_with(sessions_root) || backend.session_path == sessions_root {
        return Err(format!(
            "The real task session escaped its job-owned root: {}",
            backend.session_path.display()
        ));
    }
    let state: Value = serde_json::from_slice(
        &fs::read(backend.session_path.join("state.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let session_id = state
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            format!(
                "The real session state does not identify its ledger: {}",
                backend.session_path.display()
            )
        })?
        .to_string();
    let state_cwd = state.get("cwd").and_then(Value::as_str).unwrap_or_default();
    if fs::canonicalize(state_cwd).ok() != fs::canonicalize(workspace_root).ok() {
        return Err(
            "The real task must record the job-owned workspace, not an operator workspace"
                .to_string(),
        );
    }
    let transcript = fs::read_to_string(backend.session_path.join("transcript.jsonl"))
        .map_err(|error| error.to_string())?;
    let events: Vec<Value> = transcript
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .map(|event| event.get("payload").cloned().unwrap_or(event))
                .map_err(|error| error.to_string())
        })
        .collect::<Result<_, _>>()?;
    let contracts: Vec<&Value> = events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("task_contract"))
        .collect();
    if contracts.len() != 1 {
        return Err("The real task must retain one task_contract event".to_string());
    }
    let contract_data = contracts[0].get("data").unwrap_or(&Value::Null);
    if contract_data.get("task").and_then(Value::as_str) != Some(task) {
        return Err("The retained task contract must name the actual task".to_string());
    }
    if contract_data.get("version") != backend.contract.get("version") {
        return Err(
            "The retained task contract must use the version Settings received".to_string(),
        );
    }
    if contract_data.get("instructions") != backend.contract.get("instructions") {
        return Err(
            "The retained task contract must use the instructions Settings received".to_string(),
        );
    }
    if sorted_requirement_ids(contract_data) != required_ids() {
        return Err(
            "The retained task contract must contain every required report entry".to_string(),
        );
    }
    let reports: Vec<&Value> = events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("task_report"))
        .collect();
    if reports.len() != 1 {
        return Err("The real task must retain exactly one task_report event".to_string());
    }
    let report = reports[0].get("data").cloned().unwrap_or(Value::Null);
    if report.get("version") != backend.contract.get("version") {
        return Err("The task report must use the contract Settings shows".to_string());
    }
    if report.get("status").and_then(Value::as_str) != Some("complete") {
        return Err("A blocked task report cannot satisfy the native journey".to_string());
    }
    let mut report_keys: Vec<String> = report
        .get("report")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|map| map.keys().cloned())
        .collect();
    report_keys.sort();
    if report_keys != required_ids() {
        return Err("The task report must answer exactly the seven required entries".to_string());
    }
    for (requirement, entry) in report
        .get("report")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let status = entry
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !matches!(status, "done" | "not_applicable") {
            return Err(format!(
                "{requirement} must be done or honestly inapplicable"
            ));
        }
        if entry
            .get("explanation")
            .and_then(Value::as_str)
            .is_none_or(|text| text.trim().is_empty())
        {
            return Err(format!("{requirement} must have a concrete explanation"));
        }
        let evidence = entry
            .get("evidence")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{requirement} evidence must be an array"))?;
        if status == "done"
            && !evidence.iter().any(|reference| {
                reference
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty())
            })
        {
            return Err(format!("{requirement} marked done must cite real evidence"));
        }
    }
    if events
        .iter()
        .any(|event| event.get("type").and_then(Value::as_str) == Some("tool_call"))
    {
        return Err("The read-only report task must not invoke a product tool".to_string());
    }
    let finals: Vec<&Value> = events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("final"))
        .collect();
    if finals.len() != 1 {
        return Err("The one-turn report session must retain one final event".to_string());
    }
    let final_text = finals[0]
        .pointer("/data/text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !final_text.contains(task_marker) {
        return Err("The durable final answer must contain the native report marker".to_string());
    }
    let report_text = report
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| "The task report must render non-empty report text".to_string())?;
    if final_text.matches(report_text).count() != 1 {
        return Err(
            "The durable final answer must contain the rendered task report exactly once"
                .to_string(),
        );
    }
    if events.iter().any(|event| {
        event.get("type").and_then(Value::as_str) == Some("contract_violation")
            && event.pointer("/data/outcome").and_then(Value::as_str) == Some("rejected")
    }) {
        return Err(
            "A rejected contract violation cannot satisfy the native report journey".to_string(),
        );
    }
    trace.push(json!({"label":"recorded-task-report","sessionPath":backend.session_path,"sessionID":session_id,"task":task,"report":report,"finalText":final_text}));
    Ok(Recorded {
        session_id,
        report,
        final_text,
    })
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn assert_contract_visible(tree: &str, contract: &Value) -> Result<(), String> {
    if !tree.contains("id=task-contract-instructions") {
        return Err(
            "Settings must expose the loaded contract instructions with task-contract-instructions"
                .to_string(),
        );
    }
    if !tree.contains("id=task-contract-requirements") {
        return Err(
            "Settings must expose the loaded requirements with task-contract-requirements"
                .to_string(),
        );
    }
    if tree.contains("id=task-contract-unavailable") {
        return Err(
            "Settings must not expose task-contract-unavailable after the real RPC contract loads"
                .to_string(),
        );
    }
    let version = contract
        .get("version")
        .map(Value::to_string)
        .unwrap_or_default();
    if !tree.contains(&format!("VERSION {version}")) {
        return Err("Settings must show the backend contract version".to_string());
    }
    let values: HashSet<String> = common::static_texts(tree)
        .into_iter()
        .map(|value| normalize(&value))
        .collect();
    let instructions = contract
        .get("instructions")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !values.contains(&normalize(instructions)) {
        return Err(
            "Settings must show the complete instructions returned by the real backend".to_string(),
        );
    }
    let requirements = contract
        .get("requirements")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for requirement in &requirements {
        let id = requirement
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let title = requirement
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let description = requirement
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !values.contains(id) {
            return Err(format!("Settings must visibly render requirement id {id}"));
        }
        if !values.contains(&normalize(title)) {
            return Err(format!("Settings must visibly render the {id} title"));
        }
        if !values.contains(&normalize(description)) {
            return Err(format!(
                "Settings must visibly render the complete {id} requirement returned by Jeden"
            ));
        }
    }
    if requirements.len() != 7 {
        return Err("The loaded task contract must define exactly seven requirements".to_string());
    }
    Ok(())
}

fn wait_for_session(
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

fn wait_for_report(
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

fn assert_report_visible(tree: &str, contract: &Value, recorded: &Recorded) -> Result<(), String> {
    if !tree.contains("id=conversation-entry-final") {
        return Err("Conversation must identify the durable final answer it rendered".to_string());
    }
    if tree.contains("id=conversation-entry-task_report") {
        return Err("Conversation must not render the task_report beside the final answer that already contains it".to_string());
    }
    let values = common::static_texts(tree);
    let expected = normalize(&recorded.final_text);
    if values
        .iter()
        .filter(|value| normalize(value) == expected)
        .count()
        != 1
    {
        return Err("Conversation must render the durable final answer exactly once".to_string());
    }
    let rendered_report = normalize(
        recorded
            .report
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let occurrences: usize = values
        .iter()
        .map(|value| normalize(value).matches(&rendered_report).count())
        .sum();
    if occurrences != 1 {
        return Err(
            "The real seven-point task report must appear exactly once in the native conversation"
                .to_string(),
        );
    }
    for requirement in contract
        .get("requirements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let id = requirement
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let title = requirement
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let entry = recorded
            .report
            .pointer(&format!("/report/{id}"))
            .unwrap_or(&Value::Null);
        let status = match entry.get("status").and_then(Value::as_str) {
            Some("not_applicable") => "not applicable",
            Some(status) => status,
            None => "",
        };
        let explanation = entry
            .get("explanation")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if !expected.contains(&normalize(&format!("{title} ({status}): {explanation}"))) {
            return Err(format!(
                "The native final answer must include the real {id} report explanation"
            ));
        }
        for evidence in entry
            .get("evidence")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !expected.contains(&normalize(evidence)) {
                return Err(format!(
                    "The native final answer must include the real {id} evidence reference"
                ));
            }
        }
    }
    Ok(())
}

fn publish_trace(context: &specs::Context, trace: &[Value]) -> Result<(), String> {
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
