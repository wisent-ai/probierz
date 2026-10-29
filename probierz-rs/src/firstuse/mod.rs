//! A product's first-use journey, run the way a new user meets it.
//!
//! `probierz run first-use/<app_id>` reads the app's `first-use.yaml` beside
//! its manifest (`declare`), gives the run a clean home of its own (`play`),
//! installs the product from its public install instructions, walks the
//! screens of its first-use definition in the order a user meets them
//! (`walk`), runs the command each screen shows with the user's own data, and
//! reads the first-success fact where the product records the journey.
//!
//! The record lands at `test-results/<app_id>/first-use/<started>/record.json`
//! beside every step's output, and is printed. The run fails unless the
//! product recorded the first-success fact with the value the definition
//! expects: an install or a shown command that did not succeed, or a fact
//! the product never recorded, is a failed first use, named in the record.

mod declare;
mod play;
mod walk;

use std::path::Path;

use serde_json::{json, Value};

use crate::failure::{now_iso, print_json, Answer, Code, Failure};

/// The target prefix that names a product's first-use journey to `probierz run`.
pub(crate) const TARGET_PREFIX: &str = "first-use/";

pub(crate) fn run(harness: &Path, app_id: &str) -> Answer {
    let manifest = crate::manifest::load(harness, app_id)?;
    let declared = declare::declared(&manifest)?;
    let started = now_iso();
    let root = harness
        .join("test-results")
        .join(&manifest.app_id)
        .join("first-use")
        .join(started.replace([':', '.'], "-"));
    let sandbox = play::Sandbox::create(root.clone(), &declared)?;
    let mut record = play::play(&declared, &sandbox)?;
    record["appId"] = json!(manifest.app_id);
    record["journey"] = declared.describe();
    let file = root.join("record.json");
    std::fs::write(&file, serde_json::to_vec_pretty(&record)?)?;
    print_json(&record)?;
    if record["passed"] == true {
        return Ok(());
    }
    Err(Failure::new(
        "first-use.run",
        Code::Refused,
        format!(
            "{}'s first use did not reach its first-success fact {} (installed: {}, walked: {}); the record names the step: {}",
            manifest.app_id,
            record["firstSuccess"]["fact"],
            record["installed"],
            record["walked"],
            file.display()
        ),
    ))
}

/// Where an app's first-use journey stands, for the fleet overview:
/// `undeclared` without a `first-use.yaml`, `never-run` when no run left a
/// directory, `unrecorded` when the newest run stopped before writing its
/// record, otherwise the newest record's `passed` or `failed`, with the
/// directory of that run. Run directories are named by their start time, so
/// the greatest name is the newest run.
pub(crate) fn progress(harness: &Path, app_id: &str) -> Result<Value, Failure> {
    let manifest = crate::manifest::load(harness, app_id)?;
    if !declare::is_declared(&manifest) {
        return Ok(json!({ "state": "undeclared" }));
    }
    let runs = harness
        .join("test-results")
        .join(&manifest.app_id)
        .join("first-use");
    let entries = match std::fs::read_dir(&runs) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({ "state": "never-run" }));
        }
        Err(error) => {
            return Err(Failure::config(
                "first-use.progress",
                format!("cannot list {}: {error}", runs.display()),
            ))
        }
    };
    let mut newest = None;
    for entry in entries {
        let path = entry
            .map_err(|error| {
                Failure::config(
                    "first-use.progress",
                    format!("cannot list {}: {error}", runs.display()),
                )
            })?
            .path();
        if path.is_dir()
            && newest
                .as_ref()
                .is_none_or(|seen: &std::path::PathBuf| path > *seen)
        {
            newest = Some(path);
        }
    }
    let Some(run) = newest else {
        return Ok(json!({ "state": "never-run" }));
    };
    let record = run.join("record.json");
    let text = match std::fs::read_to_string(&record) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({ "state": "unrecorded", "run": run.display().to_string() }));
        }
        Err(error) => {
            return Err(Failure::config(
                "first-use.progress",
                format!("cannot read {}: {error}", record.display()),
            ))
        }
    };
    let value: Value = serde_json::from_str(&text).map_err(|error| {
        Failure::config(
            "first-use.progress",
            format!("{} is not JSON: {error}", record.display()),
        )
    })?;
    let state = if value["passed"] == true {
        "passed"
    } else {
        "failed"
    };
    Ok(json!({ "state": state, "run": run.display().to_string() }))
}
