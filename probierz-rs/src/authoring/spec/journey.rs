use crate::authoring::*;
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub fn author_spec(
    harness: &Path,
    app_id: &str,
    journey: &str,
    target: &str,
    desc: &str,
    base_url: Option<&str>,
    app_path: Option<&str>,
    mapping_paths: &[String],
    rounds: u32,
    dry_run: bool,
) -> Result<JsonValue, Failure> {
    let Some(directory) = target_spec_dir(harness, target) else {
        return Err(if matches!(target, "tui" | "desktop:cua" | "web") {
            registry_surface_refusal("author-spec", target)
        } else {
            Failure::invalid("author-spec", format!("unsupported target: {target}"))
        });
    };
    let loaded = manifest::load(harness, app_id)?;
    if loaded
        .document
        .get("surfaces")
        .and_then(|surfaces| surfaces.get(target))
        .is_none()
    {
        return Err(Failure::config(
            "author-spec",
            format!("app {app_id} has no {target} surface"),
        ));
    }
    if app_path.is_none() {
        return Err(Failure::invalid(
            "author-spec",
            format!("{target} authoring needs --app-path"),
        ));
    }
    let probe = probe(target, base_url, app_path)
        .map_err(|detail| Failure::unavailable("author-spec.probe", detail))?;
    let staged = directory.join(format!(".author-staging-{journey}{}", SPEC_EXTENSION));
    fs::create_dir_all(&directory)?;
    let first_brief =
        author_spec_brief(app_id, journey, target, desc, &probe, 1, rounds, None, &[]);
    if dry_run {
        return Ok(
            json!({ "ok": true, "dryRun": true, "brief": first_brief, "stagedPath": staged.to_string_lossy() }),
        );
    }
    let mut previous: Option<String> = None;
    let mut failures: Vec<String> = Vec::new();
    for round in 1..=rounds {
        let brief = if round == 1 {
            first_brief.clone()
        } else {
            author_spec_brief(
                app_id,
                journey,
                target,
                desc,
                &probe,
                round,
                rounds,
                previous.as_deref(),
                &failures,
            )
        };
        let _ = fs::remove_file(&staged);
        let drafted = match draft_structured_artifact(
            harness,
            app_id,
            Some(target),
            &brief,
            "submit_probierz_spec",
            "Submit the complete Probierz journey spec for the current authoring round.",
        ) {
            Ok(value) => value,
            Err(detail) => {
                return Ok(
                    json!({ "ok": false, "reason": "Stado model-router authoring failed", "detail": detail }),
                )
            }
        };
        fs::write(&staged, drafted.content).map_err(|cause| {
            Failure::config(
                "author-spec",
                format!("writing staged draft {}: {cause}", staged.display()),
            )
        })?;
        let run = run_authored_spec(harness, app_id, target, &staged, base_url, app_path)?;
        if run["passed"] == true {
            let accepted =
                install_accepted_spec(harness, app_id, journey, target, &staged, mapping_paths)?;
            return Ok(json!({
                "ok": true,
                "journey": journey,
                "target": target,
                "spec": accepted["spec"],
                "manifest": accepted["manifest"],
                "runId": run["runId"],
                "rounds": round,
            }));
        }
        // The next round is briefed with this draft; one that cannot be read
        // back fails authoring with its cause instead of briefing blind.
        previous = Some(fs::read_to_string(&staged).map_err(|cause| {
            Failure::config(
                "author-spec",
                format!("reading staged draft {}: {cause}", staged.display()),
            )
        })?);
        failures = run["failures"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(JsonValue::as_str)
            .map(str::to_string)
            .collect();
    }
    let _ = fs::remove_file(&staged);
    Ok(
        json!({ "ok": false, "reason": format!("authoring did not converge in {rounds} rounds"), "lastFailures": failures }),
    )
}

/// The top of the repository as the authoring model sees it. A root or entry
/// that cannot be read is written into the listing with its cause, so the
/// model, and whoever reads its transcript, is not shown "(unreadable)" or a
/// silently shorter tree.
pub(crate) fn repo_tree(root: &Path) -> String {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => return format!("({} cannot be listed: {error})", root.display()),
    };
    let mut unreadable = Vec::new();
    let mut entries: Vec<_> = entries
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(error) => {
                unreadable.push(format!("(an entry cannot be read: {error})"));
                None
            }
        })
        .take(40)
        .collect();
    entries.sort_by_key(|entry| entry.file_name());

    let mut lines: Vec<String> = entries
        .into_iter()
        .map(|entry| {
            format!(
                "{}{}",
                entry.file_name().to_string_lossy(),
                if entry.path().is_dir() { "/" } else { "" }
            )
        })
        .collect();
    lines.extend(unreadable);
    lines.join("\n")
}
pub(crate) fn run_authored_spec(
    harness: &Path,
    app_id: &str,
    target: &str,
    staged: &Path,
    base_url: Option<&str>,
    app_path: Option<&str>,
) -> Result<JsonValue, Failure> {
    let executable = std::env::current_exe()?;
    let mut command = Command::new(executable);
    command
        .arg("--harness")
        .arg(harness)
        .args(["run", target, "--app", app_id, "--spec"])
        .arg(staged)
        .arg("--no-repair")
        .arg("PROBIERZ_RUN_KIND=pull-request")
        .env("PROBIERZ_REPAIR_SUPPRESS", "1");
    if let Some(value) = base_url {
        command.arg(format!("BASE_URL={value}"));
    }
    if let Some(value) = app_path {
        let name = if target.starts_with("mobile:") {
            "APP_IOS"
        } else if target == "tui" {
            "TUI_CMD"
        } else if target == "desktop:cua" {
            "CUA_APP_EXECUTABLE"
        } else {
            "MAC_APP_PATH"
        };
        command.arg(format!("{name}={value}"));
    }
    let output = command.output()?;
    if output.stdout.is_empty() {
        return Ok(json!({
            "passed": false,
            "status": "unknown",
            "runId": JsonValue::Null,
            "failures": [String::from_utf8_lossy(&output.stderr)],
        }));
    }
    let value: JsonValue = serde_json::from_slice(&output.stdout)?;
    let failures = value
        .pointer("/analysis/failures")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(|failure| {
            failure
                .get("error")
                .or_else(|| failure.get("message"))
                .and_then(JsonValue::as_str)
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    Ok(json!({
        "passed": value.get("passed").and_then(JsonValue::as_bool).unwrap_or_else(|| value.get("status").and_then(JsonValue::as_str) == Some("passed")),
        "status": value.get("status").and_then(JsonValue::as_str).unwrap_or("unknown"),
        "runId": value.get("runId").cloned().unwrap_or(JsonValue::Null),
        "failures": failures,
    }))
}
