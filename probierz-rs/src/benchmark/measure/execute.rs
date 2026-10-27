//! Running one contender on one case: the task goes in on stdin, the result
//! comes back on stdout, and the clock measures the whole process.
//!
//! A contender starts with an empty environment plus exactly the variables
//! its declaration names in `env`, so one rival never reads another's
//! credentials and a run depends on nothing it did not declare.

use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value as Json};

use crate::benchmark::inputs::declare::Contender;
use crate::benchmark::inputs::suite::{filled, Case, Loaded};
use crate::benchmark::{RESULT_SCHEMA, TASK_SCHEMA};
use crate::failure::{ended, Code, Failure};

/// What a contender answers. Unknown keys are refused, so a contender that
/// misspells a measurement is told so rather than silently scored without it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Answer {
    schema: String,
    status: AnswerStatus,
    #[serde(default)]
    output: Json,
    steps: Option<u64>,
    tokens: Option<u64>,
    cost_usd: Option<f64>,
    error: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum AnswerStatus {
    Completed,
    Failed,
}

/// Refuse a contender that cannot run before any case is spent on it.
pub(crate) fn ready(contender: &Contender) -> Result<(), Failure> {
    let metadata = std::fs::metadata(&contender.program).map_err(|error| {
        Failure::new(
            "benchmark.contender",
            Code::Prerequisite,
            format!(
                "contender {} program {} cannot be read ({error}); install it or correct benchmark.contenders.{}.program",
                contender.id,
                contender.program.display(),
                contender.id
            ),
        )
    })?;
    // Executable by anyone: the owner, group or other execute bit is set,
    // so clearing them changes the mode.
    let execute = u32::from(libc::S_IXUSR | libc::S_IXGRP | libc::S_IXOTH);
    let mode = metadata.permissions().mode();
    if !metadata.is_file() || mode & !execute == mode {
        return Err(Failure::new(
            "benchmark.contender",
            Code::Prerequisite,
            format!(
                "contender {} program {} is not an executable file",
                contender.id,
                contender.program.display()
            ),
        ));
    }
    for name in &contender.env {
        if std::env::var_os(name).is_none() {
            return Err(Failure::new(
                "benchmark.contender",
                Code::Prerequisite,
                format!(
                    "contender {} declares variable {name}, which is not set in this environment",
                    contender.id
                ),
            ));
        }
    }
    Ok(())
}

/// One measured attempt, judged later by `assess`.
pub(crate) struct Attempt {
    pub duration_ms: u64,
    pub ended: String,
    pub broken: Option<String>,
    pub completed: bool,
    pub output: Json,
    pub steps: Option<u64>,
    pub tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub error: Option<String>,
}

fn broken(duration_ms: u64, ended: String, reason: String) -> Attempt {
    Attempt {
        duration_ms,
        ended,
        broken: Some(reason),
        completed: false,
        output: Json::Null,
        steps: None,
        tokens: None,
        cost_usd: None,
        error: None,
    }
}

/// What a stream said, whole: a cut at a length nobody declared dropped the
/// line that said why the contender broke whenever it came early.
fn said(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

pub(crate) fn attempt(
    contender: &Contender,
    loaded: &Loaded,
    values: &BTreeMap<String, String>,
    case: &Case,
    repetition: usize,
) -> Result<Attempt, Failure> {
    let task = json!({
        "schema": TASK_SCHEMA,
        "suite": {"id": loaded.suite.id, "version": loaded.suite.version, "hash": loaded.hash},
        "case": {"id": case.id, "instruction": case.instruction, "input": filled(&case.input, values)},
        "repetition": repetition,
    });
    let mut command = Command::new(&contender.program);
    command
        .args(&contender.args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in &contender.env {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    // Encoded before the contender starts, so a task that cannot be encoded
    // never leaves a started contender behind.
    let task_bytes = serde_json::to_vec(&task)?;
    let label = format!("contender {} ({})", contender.id, contender.program.display());
    let started = Instant::now();
    let mut child = command.spawn().map_err(|error| {
        Failure::new(
            "benchmark.contender",
            Code::Prerequisite,
            format!(
                "contender {} could not start {}: {error}",
                contender.id,
                contender.program.display()
            ),
        )
    })?;
    let Some(mut stdin) = child.stdin.take() else {
        // No handle to hand it the task: the contender is waited for, so it
        // is not left running, and the attempt is refused with how it ended.
        let reaped = stado_wait::child_output(child, label)?;
        return Err(Failure::new(
            "benchmark.contender",
            Code::Unknown,
            format!(
                "contender {} started without a stdin pipe to receive its task; it {}",
                contender.id,
                ended(&reaped.status)
            ),
        ));
    };
    // The task is written from its own thread while the contender's output is
    // read: written first, a contender that answers before reading all of a
    // large task filled its output pipe and both waited forever.
    let writer = std::thread::spawn(move || stdin.write_all(&task_bytes));
    let output = stado_wait::child_output(child, label)?;
    // A writer that panicked is this process's own fault: its panic goes on.
    let written = match writer.join() {
        Ok(written) => written,
        Err(payload) => std::panic::resume_unwind(payload),
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    let ended = ended(&output.status);
    if let Err(error) = written {
        return Ok(broken(
            duration_ms,
            ended,
            format!("the contender closed stdin before reading the task: {error}"),
        ));
    }
    if !output.status.success() {
        let reason = format!("the contender {ended}; stderr: {}", said(&output.stderr));
        return Ok(broken(duration_ms, ended, reason));
    }
    let answer: Answer = match serde_json::from_slice(&output.stdout) {
        Ok(answer) => answer,
        Err(error) => {
            let reason = format!(
                "stdout is not a {RESULT_SCHEMA} document: {error}; stdout: {}",
                said(&output.stdout)
            );
            return Ok(broken(duration_ms, ended, reason));
        }
    };
    if answer.schema != RESULT_SCHEMA {
        let reason = format!(
            "the result's schema is {}, expected {RESULT_SCHEMA}",
            answer.schema
        );
        return Ok(broken(duration_ms, ended, reason));
    }
    Ok(Attempt {
        duration_ms,
        ended,
        broken: None,
        completed: answer.status == AnswerStatus::Completed,
        output: answer.output,
        steps: answer.steps,
        tokens: answer.tokens,
        cost_usd: answer.cost_usd,
        error: answer.error,
    })
}
