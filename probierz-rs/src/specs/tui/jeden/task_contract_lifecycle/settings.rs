//! The two ways the contract's settings are read and written — the CLI
//! and the RPC — and the refusals each owes.
//!
//! Both write the same file, so the journey writes through one and
//! reads through the other. A refusal must change nothing: the
//! settings file is compared byte-for-byte across it.

use super::*;

/// Where the settings this journey reads and writes live, under the
/// isolated HOME.
const SETTINGS: &str = ".jeden/config.yml";

/// Two communication contracts written through the CLI, one after the
/// other, so persistence is shown rather than a single value.
const COMMUNICATION_VALUES: [&str; 2] = ["Use plain sentences.", "Answer in Polish."];

/// The CLI path: set, read back, reset, and refuse an unknown key.
pub(crate) fn check_cli_settings(
    binary: &str,
    workspace: &Path,
    home: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    for value in COMMUNICATION_VALUES {
        run_config(
            binary,
            &["config", "set", "contracts.communication", value],
            workspace,
            env,
        )?;
        if setting(home, "communication")? != value {
            return Err("CLI contract communication did not persist".into());
        }
        let read = run_config(
            binary,
            &["config", "get", "contracts.communication"],
            workspace,
            env,
        )?;
        if read.stdout.trim() != value {
            return Err("CLI config get returned a different communication value".into());
        }
    }

    let functionality = "Complete the requested operation.";
    run_config(
        binary,
        &["config", "set", "contracts.functionality", functionality],
        workspace,
        env,
    )?;
    if setting(home, "functionality")? != functionality {
        return Err("CLI contract functionality did not persist".into());
    }
    run_config(
        binary,
        &["config", "reset", "contracts.functionality"],
        workspace,
        env,
    )?;
    if setting(home, "functionality")? != "" {
        return Err("CLI contract functionality did not reset".into());
    }

    let before = settings_bytes(home)?;
    let refused = command(
        binary,
        &common::strings(&["config", "get", "contracts.style"]),
        workspace,
        env,
        None,
        COMMAND_TIMEOUT,
    )?;
    if refused.code() != Some(REFUSED_STATUS)
        || refused.stderr.trim() != "Error: unknown config key: contracts.style"
        || settings_bytes(home)? != before
    {
        return Err("unknown config key refusal changed settings or answered incorrectly".into());
    }
    Ok(())
}

/// The RPC path: read the contract, write both settings at once, prove
/// the CLI reads what RPC wrote, and refuse a partial write whole.
pub(crate) fn check_rpc_settings(
    binary: &str,
    workspace: &Path,
    home: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let initial = rpc(binary, "config/contracts/get", json!({}), workspace, env)?;
    if !initial["error"].is_null() {
        return Err("config/contracts/get returned an error".into());
    }
    check_contract(&initial["result"]["taskContract"])?;

    let saved = rpc(
        binary,
        "config/contracts/set",
        json!({"communication":"Be concise.","functionality":"Finish the task."}),
        workspace,
        env,
    )?;
    if !saved["error"].is_null() {
        return Err("config/contracts/set returned an error".into());
    }
    check_contract(&saved["result"]["taskContract"])?;
    if setting(home, "communication")? != "Be concise."
        || setting(home, "functionality")? != "Finish the task."
    {
        return Err("RPC contract settings did not persist".into());
    }

    let read = run_config(
        binary,
        &["config", "get", "contracts.functionality"],
        workspace,
        env,
    )?;
    if read.stdout.trim() != "Finish the task." {
        return Err("CLI did not read the functionality saved through RPC".into());
    }

    let before = settings_bytes(home)?;
    let refused = rpc(
        binary,
        "config/contracts/set",
        json!({"communication":"Incomplete request."}),
        workspace,
        env,
    )?;
    if refused["error"]["code"] != "invalid_params"
        || refused["error"]["message"] != "functionality must be a string"
        || settings_bytes(home)? != before
    {
        return Err("invalid RPC settings were not refused atomically".into());
    }
    Ok(())
}

/// Run one config command and require it to succeed.
fn run_config(
    binary: &str,
    args: &[&str],
    workspace: &Path,
    env: &BTreeMap<String, String>,
) -> Result<common::Output, String> {
    let result = command(
        binary,
        &common::strings(args),
        workspace,
        env,
        None,
        COMMAND_TIMEOUT,
    )?;
    if !result.status.success() {
        return Err(result.combined());
    }
    Ok(result)
}

/// One contract setting, as the settings file holds it.
fn setting(home: &Path, name: &str) -> Result<String, String> {
    let settings = common::read_json(&home.join(SETTINGS))?;
    Ok(settings["contracts"][name]
        .as_str()
        .unwrap_or_default()
        .to_string())
}

fn settings_bytes(home: &Path) -> Result<Vec<u8>, String> {
    fs::read(home.join(SETTINGS)).map_err(|e| e.to_string())
}
