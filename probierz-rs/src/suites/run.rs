//! Running the selected suites in the checkout and keeping what each did.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::{json, Value};

use crate::failure::{now_iso, Failure};

use super::manifest::{Selected, TestGate};
use super::{identity, record};

/// One finished run: its document, where it is, and how it ended.
pub(crate) struct Recorded {
    pub(crate) document: Value,
    pub(crate) directory: PathBuf,
    pub(crate) failed: bool,
    pub(crate) failed_count: usize,
    pub(crate) gate_count: usize,
}

fn passed(outcome: &Value) -> bool {
    outcome.get("passed") == Some(&Value::Bool(true))
}

/// Run every selected gate in the checkout; the run directory holds one
/// `run.json` and, per gate, its two streams and its copied reports.
pub(crate) fn run(repo: &Path, selected: &Selected, root: &Path) -> Result<Recorded, Failure> {
    let repo = repo.canonicalize().map_err(|error| {
        Failure::config(
            "suites.run",
            format!(
                "{} is not a checkout this host can open: {error}",
                repo.display()
            ),
        )
    })?;
    let source = identity::read(&repo)?;
    let started_at = now_iso();
    let directory = record::create_run_directory(root, &started_at, &source.short_revision)?;
    let mut gates = Vec::new();
    for gate in &selected.gates {
        gates.push(run_gate(&repo, gate, &directory)?);
    }
    let gate_count = gates.len();
    let failed_count = gates.iter().filter(|outcome| !passed(outcome)).count();
    let failed = gates.iter().any(|outcome| !passed(outcome));
    let document = json!({
        "schema": super::RUN_SCHEMA,
        "product": selected.product,
        "source": source.document(&repo),
        "startedAt": started_at,
        "finishedAt": now_iso(),
        "directory": directory,
        "gates": gates,
        "verdict": if failed { "failed" } else { "passed" },
    });
    record::write_run(&directory, &document)?;
    Ok(Recorded {
        document,
        directory,
        failed,
        failed_count,
        gate_count,
    })
}

/// One gate: spawned in the checkout with its streams on files, waited for to
/// its own end, its reports copied beside them. A gate that cannot start is
/// recorded with the error and counts as failed.
fn run_gate(repo: &Path, gate: &TestGate, directory: &Path) -> Result<Value, Failure> {
    let gate_dir = directory.join(&gate.name);
    std::fs::create_dir_all(&gate_dir).map_err(|error| {
        Failure::config(
            "suites.run",
            format!("{} could not be created: {error}", gate_dir.display()),
        )
    })?;
    let stdout_path = gate_dir.join("stdout.log");
    let stderr_path = gate_dir.join("stderr.log");
    let stdout = File::create(&stdout_path)?;
    let stderr = File::create(&stderr_path)?;
    let started_at = now_iso();
    let started = Instant::now();
    let Some((program, arguments)) = gate.argv.split_first() else {
        return Err(Failure::config(
            "suites.run",
            format!("gate {:?} declares an empty argv", gate.name),
        ));
    };
    let mut command = Command::new(program);
    command
        .args(arguments)
        .current_dir(repo)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Ok(json!({
                "name": gate.name,
                "argv": gate.argv,
                "cwd": repo,
                "startedAt": started_at,
                "passed": false,
                "error": format!("{program:?} could not start in {}: {error}", repo.display()),
                "stdout": stdout_path,
                "stderr": stderr_path,
                "reports": [],
            }));
        }
    };
    let pid = child.id();
    let waited = stado_wait::child_output(
        child,
        format!(
            "waiting for suite {} ({}) in {}",
            gate.name,
            gate.argv.join(" "),
            repo.display()
        ),
    )
    .map_err(|error| {
        Failure::unavailable(
            "suites.run",
            format!(
                "waiting for suite {} process {pid} failed: {error}",
                gate.name
            ),
        )
    })?;
    let duration = started.elapsed().as_secs_f64();
    let status = waited.status;
    let reports = copy_reports(repo, gate, &gate_dir)?;
    Ok(json!({
        "name": gate.name,
        "argv": gate.argv,
        "cwd": repo,
        "startedAt": started_at,
        "durationSeconds": duration,
        "exitCode": status.code(),
        "signal": signal_of(&status),
        "passed": status.success(),
        "stdout": stdout_path,
        "stderr": stderr_path,
        "reports": reports,
    }))
}

#[cfg(unix)]
fn signal_of(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn signal_of(_status: &std::process::ExitStatus) -> Option<i32> {
    None
}

/// Every report path the gate declares, copied into the gate's directory as
/// the suite left it; a declared report that is not there is recorded as
/// missing, not invented.
fn copy_reports(repo: &Path, gate: &TestGate, gate_dir: &Path) -> Result<Vec<Value>, Failure> {
    let mut copied = Vec::new();
    for declared in &gate.reports {
        let source = repo.join(declared);
        let target = gate_dir.join("reports").join(declared);
        if !source.exists() {
            copied.push(json!({"declared": declared, "present": false}));
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        record::copy_tree(&source, &target)?;
        copied.push(json!({"declared": declared, "present": true, "copied": target}));
    }
    Ok(copied)
}
