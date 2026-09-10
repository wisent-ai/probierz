use serde_json::json;
use crate::authoring::*;
pub fn repair_failed_run(
    harness: &Path,
    app_id: &str,
    run_id: Option<&str>,
    rounds: u32,
    dry_run: bool,
) -> Result<JsonValue, Failure> {
    if app_id.is_empty() {
        return Ok(repair_failure(
            None,
            "config",
            false,
            "appId is required",
            "Automated repair needs an application ID.",
        ));
    }
    if !(1..=3).contains(&rounds) {
        return Ok(repair_failure(
            None,
            "config",
            false,
            format!("invalid rounds: {rounds}"),
            "Automated repair accepts one to three rounds.",
        ));
    }
    let run = match repair_source_run(harness, app_id, run_id) {
        Ok(run) => run,
        Err(result) => return Ok(result),
    };
    let run_id = run
        .get("runId")
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string();
    let repair_dir = harness
        .join("test-results")
        .join(app_id)
        .join("repairs")
        .join(&run_id);
    let result_path = repair_dir.join("result.json");
    if let Some(previous) = read_json_value(&result_path) {
        if (previous.get("ok").and_then(JsonValue::as_bool) == Some(true)
            && previous.get("dryRun").and_then(JsonValue::as_bool) != Some(true))
            || previous.get("verdict").and_then(JsonValue::as_str) == Some("not_auto_fixable")
        {
            return Ok(previous);
        }
    }
    let attempted = (|| -> Result<JsonValue, String> {
        let loaded = manifest::load(harness, app_id).map_err(|error| error.to_string())?;
        let repo_root = loaded
            .document
            .get("repositories")
            .and_then(YamlValue::as_sequence)
            .and_then(|values| values.first())
            .and_then(|value| value.get("root"))
            .and_then(YamlValue::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| "product repository is not a git checkout: missing".to_string())?;
        if !repo_root.join(".git").exists() {
            return Err(format!(
                "product repository is not a git checkout: {}",
                repo_root.display()
            ));
        }
        fs::create_dir_all(&repair_dir).map_err(|error| error.to_string())?;
        let evidence = repair_evidence(&run);
        let mut prior = None;
        for round in 1..=rounds {
            let brief = repair_brief(
                harness,
                app_id,
                &loaded,
                &run,
                &evidence,
                round,
                rounds,
                prior.as_ref(),
            );
            fs::write(repair_dir.join(format!("round-{round}-brief.txt")), &brief)
                .map_err(|error| error.to_string())?;
            if dry_run {
                let result = json!({ "ok": true, "dryRun": true, "sourceRunId": run_id, "round": round, "repairDir": repair_dir.to_string_lossy(), "brief": brief });
                write_pretty_json(&result_path, &result).map_err(|error| error.to_string())?;
                return Ok(result);
            }
            let drafted = draft_structured_artifact(harness, app_id, None, &brief, "submit_probierz_repair", "Submit one JSON repair decision with verdict, reason, explanation, patch, and spec.")?;
            let decision: JsonValue = serde_json::from_str(&drafted.content)
                .map_err(|_| "Brama repair worker returned a non-JSON decision".to_string())?;
            let verdict = decision
                .get("verdict")
                .and_then(JsonValue::as_str)
                .unwrap_or_default();
            if !matches!(verdict, "product_patch" | "spec_fix" | "not_auto_fixable") {
                return Err("Brama repair worker returned an invalid verdict".to_string());
            }
            if decision.get("reason").and_then(JsonValue::as_str).is_none()
                || decision
                    .get("explanation")
                    .and_then(JsonValue::as_str)
                    .is_none()
            {
                return Err("Brama repair worker omitted its reason or explanation".to_string());
            }
            let mut recorded = decision.clone();
            if let Some(object) = recorded.as_object_mut() {
                object.insert("routerModel".to_string(), drafted.model);
                object.insert("usage".to_string(), drafted.usage);
            }
            write_pretty_json(
                &repair_dir.join(format!("round-{round}-decision.json")),
                &recorded,
            )
            .map_err(|error| error.to_string())?;
            if verdict == "not_auto_fixable" {
                let result = json!({
                    "ok": false, "sourceRunId": run_id, "verdict": verdict,
                    "reason": decision.get("reason").and_then(JsonValue::as_str).unwrap_or("repair refused"),
                    "explanation": decision.get("explanation").and_then(JsonValue::as_str).unwrap_or(""),
                    "repairDir": repair_dir.to_string_lossy()
                });
                write_pretty_json(&result_path, &result).map_err(|error| error.to_string())?;
                return Ok(result);
            }
            if verdict == "product_patch" {
                let patch = decision
                    .get("patch")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                patch_paths(patch)?;
                fs::write(repair_dir.join(format!("round-{round}.patch")), patch)
                    .map_err(|error| error.to_string())?;
                let suffix = format!("{}-{round}", run_id.chars().take(20).collect::<String>())
                    .chars()
                    .map(|character| {
                        if character.is_ascii_alphanumeric() || character == '-' {
                            character
                        } else {
                            '-'
                        }
                    })
                    .collect::<String>();
                let message = format!(
                    "Repair Probierz run {run_id}: {}",
                    decision
                        .get("reason")
                        .and_then(JsonValue::as_str)
                        .unwrap_or_default()
                        .chars()
                        .take(120)
                        .collect::<String>()
                );
                let published = publish_repair_branch(&repo_root, &suffix, &message, |worktree| {
                    checked_process(
                        OsStr::new("git"),
                        &[OsStr::new("apply"), OsStr::new("--check"), OsStr::new("-")],
                        worktree,
                        "repair patch does not apply",
                        Some(patch),
                    )?;
                    checked_process(
                        OsStr::new("git"),
                        &[OsStr::new("apply"), OsStr::new("-")],
                        worktree,
                        "repair patch application failed",
                        Some(patch),
                    )?;
                    Ok(())
                })?;
                let mut result = json!({
                    "ok": true, "sourceRunId": run_id, "verdict": verdict,
                    "reason": decision["reason"], "explanation": decision["explanation"],
                    "repairDir": repair_dir.to_string_lossy()
                });
                if let (Some(result), Some(published)) =
                    (result.as_object_mut(), published.as_object())
                {
                    result.extend(published.clone());
                    result.insert("verification".to_string(), json!({ "status": "awaiting-build", "reason": "product patch needs the target's real build before the journey can be rerun" }));
                }
                write_pretty_json(&result_path, &result).map_err(|error| error.to_string())?;
                return Ok(result);
            }
            let spec = decision
                .get("spec")
                .and_then(JsonValue::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "spec_fix needs a complete replacement spec".to_string())?;
            let existing = recorded_spec_path(harness, &run).ok_or_else(|| {
                format!(
                    "cannot locate recorded spec {}",
                    run.get("spec")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("(missing)")
                )
            })?;
            let candidate = repair_dir.join(format!(
                "round-{round}-{}",
                existing
                    .file_name()
                    .and_then(OsStr::to_str)
                    .unwrap_or("candidate.spec")
            ));
            fs::write(&candidate, spec).map_err(|error| error.to_string())?;
            let verification = verify_repaired_spec(harness, app_id, &run, &candidate);
            if verification.get("passed").and_then(JsonValue::as_bool) != Some(true) {
                prior = Some(
                    json!({ "verdict": verdict, "reason": decision["reason"], "verification": verification }),
                );
                continue;
            }
            let relative = existing
                .strip_prefix(harness)
                .map_err(|_| {
                    format!(
                        "recorded spec is outside the Probierz repository: {}",
                        existing.display()
                    )
                })?
                .to_path_buf();
            let suffix = format!("{}-spec", run_id.chars().take(20).collect::<String>())
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() || character == '-' {
                        character
                    } else {
                        '-'
                    }
                })
                .collect::<String>();
            let published = publish_repair_branch(
                harness,
                &suffix,
                &format!("Repair Probierz spec after run {run_id}"),
                |worktree| {
                    let destination = worktree.join(&relative);
                    fs::create_dir_all(
                        destination
                            .parent()
                            .ok_or_else(|| "spec destination has no parent".to_string())?,
                    )
                    .map_err(|error| error.to_string())?;
                    fs::write(destination, spec).map_err(|error| error.to_string())
                },
            )?;
            let mut result = json!({
                "ok": true, "sourceRunId": run_id, "verdict": verdict,
                "reason": decision["reason"], "explanation": decision["explanation"],
                "repairDir": repair_dir.to_string_lossy(), "verification": verification
            });
            if let (Some(result), Some(published)) = (result.as_object_mut(), published.as_object())
            {
                result.extend(published.clone());
            }
            write_pretty_json(&result_path, &result).map_err(|error| error.to_string())?;
            return Ok(result);
        }
        let result = json!({ "ok": false, "sourceRunId": run_id, "verdict": "not_converged", "reason": format!("repair did not converge in {rounds} rounds"), "repairDir": repair_dir.to_string_lossy() });
        write_pretty_json(&result_path, &result).map_err(|error| error.to_string())?;
        Ok(result)
    })();
    match attempted {
        Ok(result) => Ok(result),
        Err(detail) => {
            let result = repair_failure(
                Some(&run_id),
                "unknown",
                false,
                &detail,
                format!("Automated repair failed: {detail}"),
            );
            if let Some(parent) = result_path.parent() {
                let _ = fs::create_dir_all(parent);
                let _ = write_pretty_json(&result_path, &result);
            }
            Ok(result)
        }
    }
}

