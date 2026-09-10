use serde_json::json;
use crate::authoring::*;

#[allow(clippy::too_many_arguments)]
pub fn author_manifest(
    harness: &Path,
    app_id: &str,
    desc: &str,
    owner: Option<&str>,
    repositories: &[String],
    target: &str,
    base_url: Option<&str>,
    app_path: Option<&str>,
    dry_run: bool,
    with_specs: bool,
) -> Result<JsonValue, Failure> {
    if repositories.is_empty() {
        return Err(Failure::invalid(
            "author-manifest",
            "authorManifest needs at least one repository",
        ));
    }
    if !matches!(target, "web" | "electron") && app_path.is_none() {
        return Err(Failure::invalid(
            "author-manifest",
            format!("{target} needs --app-path"),
        ));
    }
    if matches!(target, "web" | "electron") && base_url.is_none() {
        return Err(Failure::invalid(
            "author-manifest",
            format!("{target} needs --base-url"),
        ));
    }
    let probe = probe(target, base_url, app_path)
        .map_err(|detail| Failure::unavailable("author-manifest.probe", detail))?;
    let owner = owner
        .map(str::to_string)
        .unwrap_or_else(|| format!("{app_id} maintainers"));
    let trees = repositories
        .iter()
        .map(|root| format!("Repository {root}:\n{}", repo_tree(Path::new(root))))
        .collect::<Vec<_>>()
        .join("\n\n");
    let staged_dir = harness.join("test-results/.author-manifest");
    fs::create_dir_all(&staged_dir)?;
    let staged = staged_dir.join(format!("{app_id}.probierz.yaml"));
    let manifest_dir = harness.join("apps").join(app_id);
    let destination = manifest_dir.join("probierz.yaml");
    let build_brief = |round: u32, previous: Option<&str>, error: Option<&str>| {
        let mut value = format!(
            "Write a complete Probierz YAML app manifest.\nApplication: {app_id}\nOwner: {owner}\n\
Description: {desc}\nTarget: {target}\nRepositories: {}\n\nReal app probe:\n{probe}\n\n\
Repository trees:\n{trees}",
            repositories.join(", ")
        );
        if let (Some(previous), Some(error)) = (previous, error) {
            value.push_str(&format!("\n\nRound {round}: the previous draft FAILED validation. Fix it.\n--- DRAFT ---\n{previous}\n--- VALIDATION ERRORS ---\n{error}"));
        } else {
            value.push_str(&format!("\n\nRound {round} of 3."));
        }
        value.push_str(
            "\n\nCall submit_probierz_manifest exactly once with the complete YAML manifest.",
        );
        value
    };
    let first_brief = build_brief(1, None, None);
    if dry_run {
        return Ok(
            json!({ "ok": true, "dryRun": true, "brief": first_brief, "stagedPath": staged.to_string_lossy() }),
        );
    }
    let mut previous: Option<String> = None;
    let mut last_error: Option<String> = None;
    for round in 1..=3 {
        let brief = if round == 1 {
            first_brief.clone()
        } else {
            build_brief(round, previous.as_deref(), last_error.as_deref())
        };
        let drafted = match draft_structured_artifact(
            harness,
            app_id,
            Some(target),
            &brief,
            "submit_probierz_manifest",
            "Submit the complete YAML Probierz app manifest for the current authoring round.",
        ) {
            Ok(value) => value,
            Err(detail) => {
                return Ok(
                    json!({ "ok": false, "reason": "Stado model-router authoring failed", "detail": detail }),
                )
            }
        };
        fs::write(&staged, &drafted.content)?;
        previous = Some(drafted.content);
        let document: YamlValue =
            match serde_yaml::from_str(previous.as_deref().unwrap_or_default()) {
                Ok(value) => value,
                Err(error) => {
                    last_error = Some(error.to_string());
                    continue;
                }
            };
        if let Err(error) = manifest::validate(&document, &destination) {
            last_error = Some(error.detail);
            continue;
        }
        fs::create_dir_all(&manifest_dir)?;
        fs::write(&destination, previous.as_deref().unwrap_or_default())?;
        let mut journeys: Vec<String> = document
            .get("journeys")
            .and_then(YamlValue::as_mapping)
            .into_iter()
            .flat_map(|map| map.keys())
            .filter_map(YamlValue::as_str)
            .map(str::to_string)
            .collect();
        journeys.sort();
        let mut specs = Vec::new();
        if with_specs {
            for journey in &journeys {
                let goal = document
                    .get("journeys")
                    .and_then(|value| value.get(journey))
                    .and_then(|value| value.get("description"))
                    .and_then(YamlValue::as_str)
                    .unwrap_or(journey);
                let authored = author_spec(
                    harness,
                    app_id,
                    journey,
                    target,
                    goal,
                    base_url,
                    app_path,
                    &[],
                    3,
                    false,
                )?;
                specs.push(json!({
                    "journey": journey,
                    "ok": authored.get("ok").and_then(JsonValue::as_bool) == Some(true),
                    "spec": authored.get("spec").cloned().unwrap_or(JsonValue::Null),
                    "reason": authored.get("reason").cloned().unwrap_or(JsonValue::Null),
                }));
            }
        }
        return Ok(
            json!({ "ok": true, "appId": app_id, "manifest": destination.to_string_lossy(), "journeys": journeys, "rounds": round, "specs": specs }),
        );
    }
    Ok(
        json!({ "ok": false, "reason": format!("manifest did not validate in 3 rounds: {}", last_error.unwrap_or_else(|| "unknown validation error".to_string())) }),
    )
}

