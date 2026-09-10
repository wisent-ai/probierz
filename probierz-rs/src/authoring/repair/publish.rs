use serde_json::json;
use crate::authoring::*;
pub(crate) fn process_with_input(
    program: &OsStr,
    args: &[&OsStr],
    cwd: &Path,
    input: Option<&str>,
) -> Result<std::process::Output, String> {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    if let Some(input) = input {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "could not open child stdin".to_string())?;
        stdin
            .write_all(input.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    child.wait_with_output().map_err(|error| error.to_string())
}

pub(crate) fn checked_process(
    program: &OsStr,
    args: &[&OsStr],
    cwd: &Path,
    fallback: &str,
    input: Option<&str>,
) -> Result<std::process::Output, String> {
    let output = process_with_input(program, args, cwd, input)?;
    if output.status.success() {
        Ok(output)
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if detail.is_empty() {
            fallback.to_string()
        } else {
            detail
        })
    }
}

pub(crate) fn publish_repair_branch<F>(
    repo_root: &Path,
    suffix: &str,
    message: &str,
    mutate: F,
) -> Result<JsonValue, String>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    let branch = format!("probierz-repair/{suffix}");
    let worktree = repo_root
        .join(".worktrees")
        .join(format!("probierz-repair-{suffix}"));
    if worktree.exists() {
        return Err(format!(
            "repair worktree already exists: {}",
            worktree.display()
        ));
    }
    fs::create_dir_all(
        worktree
            .parent()
            .ok_or_else(|| "repair worktree has no parent".to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let worktree_arg = worktree.as_os_str();
    checked_process(
        OsStr::new("git"),
        &[
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("--detach"),
            worktree_arg,
            OsStr::new("HEAD"),
        ],
        repo_root,
        "git worktree add failed",
        None,
    )?;
    checked_process(
        OsStr::new("git"),
        &[OsStr::new("switch"), OsStr::new("-c"), OsStr::new(&branch)],
        &worktree,
        &format!("cannot create {branch}"),
        None,
    )?;
    mutate(&worktree)?;
    checked_process(
        OsStr::new("git"),
        &[OsStr::new("add"), OsStr::new("-A")],
        &worktree,
        "git add failed",
        None,
    )?;
    let diff = process_with_input(
        OsStr::new("git"),
        &[
            OsStr::new("diff"),
            OsStr::new("--cached"),
            OsStr::new("--quiet"),
        ],
        &worktree,
        None,
    )?;
    if diff.status.success() {
        return Err("repair produced no repository change".to_string());
    }
    checked_process(
        OsStr::new("git"),
        &[OsStr::new("commit"), OsStr::new("-m"), OsStr::new(message)],
        &worktree,
        "git commit failed",
        None,
    )?;
    let commit = checked_process(
        OsStr::new("git"),
        &[OsStr::new("rev-parse"), OsStr::new("HEAD")],
        &worktree,
        "git rev-parse failed",
        None,
    )?;
    checked_process(
        OsStr::new("git"),
        &[
            OsStr::new("push"),
            OsStr::new("-u"),
            OsStr::new("origin"),
            OsStr::new(&branch),
        ],
        &worktree,
        "git push failed",
        None,
    )?;
    let pr = process_with_input(
        OsStr::new("gh"),
        &[
            OsStr::new("pr"),
            OsStr::new("create"),
            OsStr::new("--fill"),
            OsStr::new("--head"),
            OsStr::new(&branch),
            OsStr::new("--base"),
            OsStr::new("main"),
        ],
        &worktree,
        None,
    )?;
    let pull_request = pr
        .status
        .success()
        .then(|| String::from_utf8_lossy(&pr.stdout).trim().to_string());
    let _ = process_with_input(
        OsStr::new("git"),
        &[OsStr::new("worktree"), OsStr::new("remove"), worktree_arg],
        repo_root,
        None,
    );
    Ok(
        json!({ "branch": branch, "commit": String::from_utf8_lossy(&commit.stdout).trim(), "pullRequest": pull_request }),
    )
}

pub(crate) fn verify_repaired_spec(
    harness: &Path,
    app_id: &str,
    run: &JsonValue,
    candidate: &Path,
) -> JsonValue {
    let executable = match std::env::current_exe() {
        Ok(value) => value,
        Err(error) => {
            return json!({ "passed": false, "exitCode": JsonValue::Null, "runId": JsonValue::Null, "status": "unknown", "error": error.to_string() })
        }
    };
    let target = run
        .get("target")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let output = Command::new(executable)
        .args(["--harness"])
        .arg(harness)
        .args(["run", target, "--app", app_id, "--spec"])
        .arg(candidate)
        .arg("PROBIERZ_RUN_KIND=repair")
        .env("PROBIERZ_REPAIR_SUPPRESS", "1")
        .output();
    let exit_code = output.ok().and_then(|value| value.status.code());
    let latest = crate::status::run_history_value(harness, app_id, Some(target), 1)
        .ok()
        .and_then(|history| {
            history
                .get("runs")
                .and_then(JsonValue::as_array)
                .and_then(|runs| runs.first())
                .cloned()
        });
    let status = latest
        .as_ref()
        .and_then(|run| run.get("status"))
        .and_then(JsonValue::as_str)
        .unwrap_or("unknown");
    json!({
        "passed": status == "passed",
        "exitCode": exit_code,
        "runId": latest.as_ref().and_then(|run| run.get("runId")).cloned().unwrap_or(JsonValue::Null),
        "status": status
    })
}

