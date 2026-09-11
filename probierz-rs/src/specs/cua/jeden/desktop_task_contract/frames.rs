//! The frames the journey exchanges with the real backend and the contract it
//! expects back: waiting for the contract, writing a request, the requirement
//! ids a contract must carry, and waiting for the child to exit.

use super::*;

pub(crate) fn wait_for_contract(
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

pub(crate) fn write_frame(
    stdin: &mut impl Write,
    frame: &Value,
    requests: &mut Vec<Value>,
) -> Result<(), String> {
    requests.push(frame.clone());
    writeln!(stdin, "{frame}").map_err(|error| error.to_string())?;
    stdin.flush().map_err(|error| error.to_string())
}

pub(crate) fn sorted_requirement_ids(contract: &Value) -> Vec<String> {
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

pub(crate) fn required_ids() -> Vec<String> {
    let mut ids: Vec<String> = REQUIRED_REPORT_ENTRIES
        .iter()
        .map(|value| value.to_string())
        .collect();
    ids.sort();
    ids
}

pub(crate) fn wait_child(
    child: &mut Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus, String> {
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
