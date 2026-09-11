use super::super::*;
use serde_json::json;
pub(crate) fn inspect_recorded(
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

pub(crate) fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn assert_contract_visible(tree: &str, contract: &Value) -> Result<(), String> {
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
