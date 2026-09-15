//! Running the binary: an ordinary command, one RPC exchange, and one
//! real model turn read back out of its own transcript.

use super::*;

/// How many steps a file-tool turn may take. Enough for a model to
/// read, write and read back; short enough that a turn that has lost
/// the plot ends.
const MAX_STEPS: &str = "24";

pub(crate) fn command(
    binary: &str,
    args: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    input: Option<&str>,
    timeout: Duration,
) -> Result<common::Output, String> {
    common::run(binary, args, Some(cwd), env, &[], input, timeout)
}

/// One RPC method, followed by shutdown, over a single stdio session.
pub(crate) fn rpc(
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
        COMMAND_TIMEOUT,
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

/// One real model turn, with the file tools allowed. Returns the
/// turn's own transcript events, after checking the contract, the
/// delivery report and the final answer it retained.
pub(crate) fn model_turn(
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
            MAX_STEPS.into(),
        ],
        workspace,
        env,
        None,
        LONG_TIMEOUT,
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

    let events = transcript_events(&answer, sessions)?;
    check_retained_contract(&events, task)?;
    check_delivery_report(&events)?;
    check_final_answer(&events, &answer)?;
    Ok(events)
}

/// The turn's transcript, read from the session it says it used —
/// which must be inside this run's isolated session root.
fn transcript_events(answer: &Value, sessions: &Path) -> Result<Vec<Value>, String> {
    let session = PathBuf::from(
        answer["sessionPath"]
            .as_str()
            .ok_or("sessionPath missing")?,
    );
    if !session.starts_with(sessions) || session == sessions {
        return Err("the turn must use its isolated session root".into());
    }
    let transcript =
        fs::read_to_string(session.join("transcript.jsonl")).map_err(|e| e.to_string())?;
    transcript
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str::<Value>(l).map(|e| e.get("payload").cloned().unwrap_or(e)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
