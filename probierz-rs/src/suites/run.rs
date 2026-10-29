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
        let outcome = match run_gate(&repo, gate, &directory) {
            Ok(outcome) => outcome,
            Err(error) => json!({
                "name": gate.name,
                "argv": gate.argv,
                "cwd": repo,
                "passed": false,
                "error": error.to_string(),
                "stdout": directory.join(&gate.name).join("stdout.log"),
                "stderr": directory.join(&gate.name).join("stderr.log"),
                "reports": [],
            }),
        };
        gates.push(outcome);
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

/// One gate: spawn in the checkout with its streams on files, wait for its
/// end, and copy its reports beside them. Setup, process and report errors
/// become failed gate evidence rather than hiding the rest of the run.
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
    let stdout = File::create(&stdout_path).map_err(|error| {
        Failure::config(
            "suites.run",
            format!(
                "{} could not be opened for stdout: {error}",
                stdout_path.display()
            ),
        )
    })?;
    let stderr = File::create(&stderr_path).map_err(|error| {
        Failure::config(
            "suites.run",
            format!(
                "{} could not be opened for stderr: {error}",
                stderr_path.display()
            ),
        )
    })?;
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
    let waited = match stado_wait::child_output(
        child,
        format!(
            "waiting for suite {} ({}) in {}",
            gate.name,
            gate.argv.join(" "),
            repo.display()
        ),
    ) {
        Ok(waited) => waited,
        Err(error) => {
            return Ok(json!({
                "name": gate.name,
                "argv": gate.argv,
                "cwd": repo,
                "startedAt": started_at,
                "durationSeconds": started.elapsed().as_secs_f64(),
                "passed": false,
                "error": format!("waiting for suite {} process {pid} failed: {error}", gate.name),
                "stdout": stdout_path,
                "stderr": stderr_path,
                "reports": [],
            }));
        }
    };
    let duration = started.elapsed().as_secs_f64();
    let status = waited.status;
    let reports = copy_reports(repo, gate, &gate_dir);
    Ok(json!({
        "name": gate.name,
        "argv": gate.argv,
        "cwd": repo,
        "startedAt": started_at,
        "durationSeconds": duration,
        "exitCode": status.code(),
        "signal": signal_of(&status),
        "passed": status.success() && reports.iter().all(|report| report.get("error").is_none()),
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
/// the suite left it. Missing and unreadable reports remain distinct, and a
/// failed copy does not erase the evidence from earlier reports.
fn copy_reports(repo: &Path, gate: &TestGate, gate_dir: &Path) -> Vec<Value> {
    let mut copied = Vec::new();
    for declared in &gate.reports {
        let target = gate_dir.join("reports").join(declared);
        match record::copy_report(repo, declared, &target) {
            Ok(false) => copied.push(json!({"declared": declared, "present": false})),
            Ok(true) => copied.push(json!({
                "declared": declared, "present": true, "copied": target,
            })),
            Err(error) => copied.push(json!({
                "declared": declared, "destination": target,
                "error": error.to_string(),
            })),
        }
    }
    copied
}
