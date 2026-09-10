use serde_json::json;
use crate::authoring::*;

/// Install a remotely verified candidate and update the same manifest fields as
/// the local path.
pub fn install_accepted_spec(
    harness: &Path,
    app_id: &str,
    journey: &str,
    target: &str,
    candidate: &Path,
    mapping_paths: &[String],
) -> Result<JsonValue, Failure> {
    let directory = target_spec_dir(harness, target).ok_or_else(|| {
        if matches!(target, "tui" | "desktop:cua") {
            registry_surface_refusal("author-spec.accept", target)
        } else {
            Failure::invalid(
                "author-spec.accept",
                format!("unsupported target: {target}"),
            )
        }
    })?;
    if !candidate.is_file() {
        return Err(Failure::config(
            "author-spec.accept",
            format!("accepted candidate does not exist: {}", candidate.display()),
        ));
    }
    let loaded = manifest::load(harness, app_id)?;
    let mut document = loaded.document;
    let owner = document
        .get("owner")
        .and_then(YamlValue::as_str)
        .unwrap_or("probierz")
        .to_string();
    let journeys = document
        .get_mut("journeys")
        .and_then(YamlValue::as_mapping_mut)
        .ok_or_else(|| Failure::config("author-spec.accept", "manifest journeys are required"))?;
    journeys.entry(YamlValue::from(journey)).or_insert_with(|| {
        serde_yaml::to_value(json!({ "owner": owner, "timeoutMs": 300000 }))
            .unwrap_or(YamlValue::Null)
    });
    let surface = document
        .get_mut("surfaces")
        .and_then(YamlValue::as_mapping_mut)
        .and_then(|surfaces| surfaces.get_mut(YamlValue::from(target)))
        .ok_or_else(|| {
            Failure::config(
                "author-spec.accept",
                format!("app {app_id} has no {target} surface"),
            )
        })?;
    let declared = surface
        .get_mut("journeys")
        .and_then(YamlValue::as_sequence_mut)
        .ok_or_else(|| {
            Failure::config(
                "author-spec.accept",
                format!("surface {target} journeys are required"),
            )
        })?;
    if !declared.iter().any(|value| value.as_str() == Some(journey)) {
        declared.push(YamlValue::from(journey));
        declared.sort_by(|left, right| {
            left.as_str()
                .unwrap_or_default()
                .cmp(right.as_str().unwrap_or_default())
        });
    }
    if !mapping_paths.is_empty() {
        let repositories = document
            .get_mut("repositories")
            .and_then(YamlValue::as_sequence_mut)
            .ok_or_else(|| {
                Failure::config("author-spec.accept", "manifest repositories are required")
            })?;
        let primary = repositories.first_mut().ok_or_else(|| {
            Failure::config("author-spec.accept", "manifest repositories are required")
        })?;
        let mappings = primary
            .get_mut("mappings")
            .and_then(YamlValue::as_sequence_mut)
            .ok_or_else(|| {
                Failure::config(
                    "author-spec.accept",
                    "primary repository mappings are required",
                )
            })?;
        mappings.push(serde_yaml::to_value(
            json!({ "paths": mapping_paths, "journeys": [journey] }),
        )?);
    }
    manifest::validate(&document, &loaded.file)?;
    fs::create_dir_all(&directory)?;
    let destination = directory.join(format!("{app_id}-{journey}{}", spec_extension(target)));
    fs::rename(candidate, &destination)?;
    fs::write(&loaded.file, serde_yaml::to_string(&document)?)?;
    Ok(json!({ "spec": destination.to_string_lossy(), "manifest": loaded.file.to_string_lossy() }))
}

pub(crate) fn author_spec_brief(
    app_id: &str,
    journey: &str,
    target: &str,
    desc: &str,
    probe: &str,
    round: u32,
    rounds: u32,
    previous: Option<&str>,
    failures: &[String],
) -> String {
    let mut brief = format!(
        "Write an e2e journey spec for the app \"{app_id}\" (target {target}), journey \"{journey}\".\n\
Journey goal: {desc}\n\n\
Return the complete contents of exactly one self-contained spec through the submit_probierz_spec tool.\n\
Do not use Markdown fences, modify files, or return any other artifact.\n\
Probe of the real app (use these selectors; anything else must be discovered by the spec itself):\n{probe}\n\n\
Hard rules:\n\
- Drive the real app only: no mocks, no fake selectors, no stubbing, no screenshots-only assertions.\n\
- One focused journey; readable, deterministic, no sleeps beyond explicit waits for real conditions.\n\
- The file must be self-contained and pass on the first run."
    );
    if let Some(previous) = previous.filter(|_| !failures.is_empty()) {
        brief.push_str(&format!(
            "\n\nRound {round} of {rounds}: your previous spec FAILED. Fix it based on the run failures.\n\
--- PREVIOUS SPEC ---\n{previous}\n--- RUN FAILURES ---\n{}",
            failures.join("\n")
        ));
    } else {
        brief.push_str(&format!("\n\nRound {round} of {rounds}."));
    }
    brief.push_str("\n\nCall submit_probierz_spec exactly once with the complete spec, then stop.");
    brief
}

