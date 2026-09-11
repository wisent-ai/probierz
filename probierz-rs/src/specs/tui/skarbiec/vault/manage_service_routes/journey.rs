//! The state one service-routes journey carries between its phases, the
//! commands it runs through the fixture shell, and the readers of what they leave.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::specs;
use crate::specs::tui::skarbiec::fixture::{self as fixture, CommandResult, Shell};

pub(super) const EMAIL_RESOURCE: &str = "origin:https://dash.cloudflare.com/email";
pub(super) const PASSWORD_RESOURCE: &str = "origin:https://dash.cloudflare.com/password";
pub(super) const MISSING_ITEM_RESOURCE: &str = "provider:probierz-absent-item";
pub(super) const MISSING_FIELD_RESOURCE: &str = "provider:probierz-absent-field";
pub(super) const LOGIN_ITEM: &str = "platform-admin-cloudflare";
pub(super) const SECRET_VALUE: &str = "routes-journey-secret-4b71e0";

pub(super) struct Journey<'a> {
    pub(super) context: &'a specs::Context,
    pub(super) binary: &'a str,
    pub(super) temp_dir: &'a Path,
    pub(super) routes_table: PathBuf,
    pub(super) beside_journal: PathBuf,
    pub(super) routes_audit_file: PathBuf,
    pub(super) shell: Shell,
    pub(super) executed: Vec<Value>,
}

pub(super) fn run_command(
    shell: &mut Shell,
    binary: &str,
    args: &[&str],
    env: &[(&str, &str)],
    timeout: Duration,
    label: &str,
    executed: &mut Vec<Value>,
) -> Result<CommandResult, String> {
    let result = shell.run_program(binary, args, env, timeout)?;
    executed.push(json!({"command":label,"exitStatus":result.status}));
    Ok(result)
}

pub(super) fn ok_json(
    shell: &mut Shell,
    binary: &str,
    args: &[&str],
    env: &[(&str, &str)],
    timeout: Duration,
    label: &str,
    executed: &mut Vec<Value>,
) -> Result<Value, String> {
    let result = run_command(shell, binary, args, env, timeout, label, executed)?;
    fixture::ensure(
        result.status == 0,
        format!(
            "skarbiec {} exited {}:\n{}",
            args.join(" "),
            result.status,
            tail(&result.output, 2000)
        ),
    )?;
    fixture::parse_json(&result.output, || {
        format!("skarbiec {} emitted no JSON", args.join(" "))
    })
}

pub(super) fn json_lines(path: &Path) -> Result<Vec<Value>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|error| format!("{}: {error}", path.display()))
        })
        .collect()
}

pub(super) fn backups_beside(temp_dir: &Path) -> Result<Vec<String>, String> {
    let mut backups = fs::read_dir(temp_dir)
        .map_err(|error| format!("{}: {error}", temp_dir.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("capability-routes.json.before-"))
        .collect::<Vec<_>>();
    backups.sort();
    Ok(backups)
}

pub(super) fn route_row<'a>(rows: &'a [Value], resource: &str) -> Result<&'a Value, String> {
    rows.iter()
        .find(|row| row["resource"] == resource)
        .ok_or_else(|| format!("routes list omitted {resource}"))
}

pub(super) fn tail(text: &str, limit: usize) -> String {
    let mut chars = text.chars().rev().take(limit).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}
