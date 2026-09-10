use serde_json::json;
use crate::adoption::*;

/// Parse and render the first-use journey, optionally adopting definitions first.
pub fn onboarding(project_root: &Path, arguments: &[String]) -> Answer {
    let (reset, source, replace, json) = onboarding_flags(arguments);
    if !run_onboarding(project_root, reset, source.as_deref(), replace, json)? {
        std::process::exit(1);
    }
    Ok(())
}

pub(crate) fn onboarding_flags(arguments: &[String]) -> (bool, Option<PathBuf>, bool, bool) {
    let mut reset = false;
    let mut source = None;
    let mut replace = false;
    let mut json = false;
    for (index, argument) in arguments.iter().enumerate() {
        match argument.as_str() {
            "--reset" => {
                if reset {
                    invocation_error("--reset may be supplied only once");
                }
                reset = true;
            }
            "--json" => {
                if json {
                    invocation_error("--json may be supplied only once");
                }
                json = true;
            }
            "--replace" => {
                if replace {
                    invocation_error("--replace may be supplied only once");
                }
                replace = true;
            }
            "--source" => {
                if source.is_some() {
                    invocation_error("--source may be supplied only once");
                }
                let value = arguments.get(index + 1).map(String::as_str);
                if value.is_none_or(|value| value.is_empty() || value.starts_with("--")) {
                    invocation_error("--source needs a repository path");
                }
                source = value.map(PathBuf::from);
            }
            _value if index > 0 && arguments[index - 1] == "--source" => {
                continue;
            }
            value => invocation_error(format!("unknown onboarding option: {value}")),
        }
    }
    if replace && source.is_none() {
        invocation_error("--replace requires --source <repository>");
    }
    (reset, source, replace, json)
}

pub(crate) fn run_onboarding(
    project_root: &Path,
    reset_requested: bool,
    source_root: Option<&Path>,
    replace: bool,
    json_output: bool,
) -> Result<bool, Failure> {
    if replace && source_root.is_none() {
        return Err(fail(
            "onboarding.options",
            "--replace requires --source <repository>",
        ));
    }
    let definition = onboarding_definition();
    let adoption = source_root
        .map(|source| adopt_project(project_root, source, replace))
        .transpose()?;

    let mut progress = read_progress();
    let mut reset = false;
    if reset_requested && progress.is_some() {
        progress = None;
        reset = true;
        write_initial_progress(&definition)?;
    } else if progress.is_none() {
        write_initial_progress(&definition)?;
    }

    if adoption
        .as_ref()
        .is_some_and(|value| value.get("status").and_then(Value::as_str) != Some("conflict"))
    {
        let adopted = adoption.as_ref().expect("checked");
        let mut adopted_progress = read_progress().unwrap_or_else(|| json!({}));
        let object = adopted_progress
            .as_object_mut()
            .ok_or_else(|| fail("onboarding.state", "onboarding progress is not an object"))?;
        object.insert("product_id".to_string(), definition["product_id"].clone());
        object.insert("journey_id".to_string(), definition["journey_id"].clone());
        object.insert(
            "journey_version".to_string(),
            definition["journey_version"].clone(),
        );
        object
            .entry("status".to_string())
            .or_insert_with(|| Value::String("in_progress".to_string()));
        let evidence = object
            .entry("evidence".to_string())
            .or_insert_with(|| json!({}));
        let evidence = evidence
            .as_object_mut()
            .ok_or_else(|| fail("onboarding.state", "onboarding evidence is not an object"))?;
        evidence.insert("project_definitions_adopted".to_string(), Value::Bool(true));
        object.insert(
            "adoption".to_string(),
            json!({
                "source_root": adopted["sourceRoot"],
                "source_digest": adopted["sourceDigest"],
                "accepted_at": now_iso(),
            }),
        );
        write_progress(&adopted_progress)?;
        progress = read_progress();
    }

    let done = progress
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        == Some("completed");
    let screens = ordered_screens(&definition);

    if json_output {
        let rendered: Vec<Value> = screens
            .iter()
            .map(|screen| {
                json!({
                    "screen_id": screen["screen_id"],
                    "title": screen.pointer("/presentation/title").cloned().unwrap_or_else(|| screen["title_key"].clone()),
                    "body": screen.pointer("/presentation/body").cloned().unwrap_or_else(|| screen["body_key"].clone()),
                    "command": screen.pointer("/presentation/command").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        return print_json(&json!({
            "product_id": definition["product_id"],
            "journey_id": definition["journey_id"],
            "journey_version": definition["journey_version"],
            "source_revision": definition["source_revision"],
            "first_success_fact": definition["first_success_fact"],
            "status": if done { "completed" } else { "in_progress" },
            "reset": reset,
            "adoption": adoption,
            "screens": rendered,
        }))
        .map(|_| true);
    }

    if reset {
        println!("First-run walkthrough reset: walkthrough progress and its first-success evidence discarded, showing it again now.");
        println!();
    }
    let mut accepted = true;
    if let Some(adoption) = adoption.as_ref() {
        if adoption.get("status").and_then(Value::as_str) == Some("conflict") {
            accepted = false;
            println!(
                "Existing project not adopted: {} conflicting definition(s). No files changed.",
                adoption["conflicting"]
            );
            if let Some(conflicts) = adoption.get("conflicts").and_then(Value::as_array) {
                for item in conflicts {
                    println!(
                        "       {}: {}",
                        item["path"].as_str().unwrap_or_default(),
                        item["reason"].as_str().unwrap_or_default()
                    );
                }
            }
            println!(
                "       Resolve the files or repeat with --replace after reviewing the conflicts."
            );
        } else {
            println!(
                "Existing project {}: {} imported, {} unchanged, {} removed.",
                adoption["status"].as_str().unwrap_or_default(),
                adoption["imported"],
                adoption["unchanged"],
                adoption["removed"]
            );
            println!("       Journey definitions were persisted but not run.");
        }
        println!();
    }
    for (index, screen) in screens.iter().enumerate() {
        let title = screen
            .pointer("/presentation/title")
            .or_else(|| screen.get("title_key"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let body = screen
            .pointer("/presentation/body")
            .or_else(|| screen.get("body_key"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        println!("{}/{}  {title}", index + 1, screens.len());
        println!("       {body}");
        if let Some(command) = screen
            .pointer("/presentation/command")
            .and_then(Value::as_str)
        {
            println!("       $ {command}");
        }
        println!();
    }
    let fact = definition["first_success_fact"]
        .as_str()
        .unwrap_or_default();
    if done {
        println!("First-run journey already complete: {fact} was observed on an earlier run.");
    } else {
        println!("No passing quality evidence written from this shell yet, so {fact} is still open; the next passing completed run closes it.");
    }
    Ok(accepted)
}

/// Progress recording must never interfere with the evidence operation that calls it.
pub fn record_passing_quality_evidence_written() {
    let _ = (|| -> Result<(), Failure> {
        let mut progress = read_progress().unwrap_or_else(|| json!({}));
        if progress.get("status").and_then(Value::as_str) == Some("completed") {
            return Ok(());
        }
        let definition = onboarding_definition();
        let object = progress
            .as_object_mut()
            .ok_or_else(|| fail("onboarding.state", "onboarding progress is not an object"))?;
        object.insert("product_id".to_string(), definition["product_id"].clone());
        object.insert("journey_id".to_string(), definition["journey_id"].clone());
        object.insert(
            "journey_version".to_string(),
            definition["journey_version"].clone(),
        );
        object.insert("status".to_string(), Value::String("completed".to_string()));
        let evidence = object
            .entry("evidence".to_string())
            .or_insert_with(|| json!({}));
        let evidence = evidence
            .as_object_mut()
            .ok_or_else(|| fail("onboarding.state", "onboarding evidence is not an object"))?;
        evidence.insert(
            definition["first_success_fact"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            Value::Bool(true),
        );
        object.insert("completed_at".to_string(), Value::String(now_iso()));
        write_progress(&progress)
    })();
}

