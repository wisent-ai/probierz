//! One step of the cycle: a command the operator could type himself, run to
//! its end, kept in the cycle's report with what it answered or why it
//! refused. A refused step is the cycle's evidence, not its failure: the
//! next topic, product or suite still runs.

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value as Json};

/// Owned arguments from borrowed ones.
pub(super) fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

/// A command path as the operator types it (`topic add`), one argument per
/// word; the values that follow it are pushed as they are, spaces and all.
pub(super) fn command(path: &str) -> Vec<String> {
    path.split(' ').map(str::to_string).collect()
}

/// Run one command in the cycle's harness and answer its record: the
/// arguments, the exit status, the JSON it printed and its refusal.
pub(super) fn run(program: &Path, args: &[String], harness: &Path) -> Json {
    match Command::new(program)
        .args(args)
        .env("PROBIERZ_HARNESS_DIR", harness)
        .output()
    {
        Err(error) => json!({
            "program": program.to_string_lossy(),
            "args": args,
            "ok": false,
            "refusal": format!("could not start: {error}"),
        }),
        Ok(output) => {
            let answer: Json = serde_json::from_slice(&output.stdout).unwrap_or(Json::Null);
            json!({
                "program": program.to_string_lossy(),
                "args": args,
                "exit": output.status.code(),
                "ok": output.status.success(),
                "answer": answer,
                "refusal": String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }
}

/// Whether a step's command exited successfully.
pub(super) fn ok(step: &Json) -> bool {
    step["ok"].as_bool().unwrap_or(false)
}
