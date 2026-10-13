use crate::authoring::*;
pub(crate) fn repair_brief(
    harness: &Path,
    app_id: &str,
    loaded: &manifest::Manifest,
    run: &JsonValue,
    evidence: &JsonValue,
    round: u32,
    rounds: u32,
    prior: Option<&JsonValue>,
) -> String {
    let journey = run
        .pointer("/conditions/PROBIERZ_JOURNEY")
        .and_then(JsonValue::as_str)
        .or_else(|| {
            run.get("journeys")
                .and_then(JsonValue::as_array)
                .and_then(|values| values.first())
                .and_then(JsonValue::as_str)
        });
    let repository = loaded
        .document
        .get("repositories")
        .and_then(YamlValue::as_sequence)
        .and_then(|values| values.first());
    // Fields the spec or the run left out are named as such in the brief; a
    // repair told "unknown" or nothing reads it as a value it can act on.
    let root = match repository
        .and_then(|value| value.get("root"))
        .and_then(YamlValue::as_str)
    {
        Some(root) => root,
        None => "not declared in the spec",
    };
    let mappings = repository
        .and_then(|value| value.get("mappings"))
        .and_then(YamlValue::as_sequence)
        .into_iter()
        .flatten()
        .filter(|mapping| {
            journey.is_none_or(|journey| {
                mapping
                    .get("journeys")
                    .and_then(YamlValue::as_sequence)
                    .is_some_and(|values| {
                        values.iter().any(|value| value.as_str() == Some(journey))
                    })
            })
        })
        .flat_map(|mapping| {
            mapping
                .get("paths")
                .and_then(YamlValue::as_sequence)
                .into_iter()
                .flatten()
                .filter_map(YamlValue::as_str)
        })
        .collect::<Vec<_>>();
    // A recorded spec that cannot be read is not a missing one: told "no spec
    // was found", the repair could write a spec over the one it never saw.
    let spec = match recorded_spec_path(harness, run) {
        Ok(Some(path)) => match fs::read_to_string(&path) {
            Ok(value) => format!("Current Probierz spec:\n{value}"),
            Err(error) => format!(
                "The recorded spec {} exists and could not be read ({error}); do not choose spec_fix without it.",
                path.display()
            ),
        },
        Ok(None) => "No exact spec file was found.".to_string(),
        Err(error) => format!(
            "The recorded spec could not be looked for ({error}); do not choose spec_fix without it."
        ),
    };
    let green = crate::status::run_history_value(
        harness,
        app_id,
        run.get("target").and_then(JsonValue::as_str),
        None,
        usize::MAX,
    )
    .map(|history| {
        history
            .get("runs")
            .and_then(JsonValue::as_array)
            .and_then(|runs| {
                runs.iter().find(|run| {
                    run.get("status").and_then(JsonValue::as_str) == Some("passed")
                        && journey.is_none_or(|journey| {
                            run.pointer("/conditions/PROBIERZ_JOURNEY")
                                .and_then(JsonValue::as_str)
                                == Some(journey)
                                || run
                                    .get("journeys")
                                    .and_then(JsonValue::as_array)
                                    .is_some_and(|values| {
                                        values.iter().any(|value| value.as_str() == Some(journey))
                                    })
                        })
                })
            })
            .and_then(|run| run.get("runId"))
            .and_then(JsonValue::as_str)
            .map(str::to_string)
    });
    let mut sections = vec![
        format!("Repair a recorded Probierz failure. Round {round} of {rounds}."),
        format!("Application: {app_id}"),
        format!("Repository: {root}"),
        format!(
            "Run: {}",
            match run.get("runId").and_then(JsonValue::as_str) {
                Some(id) => id,
                None => "not recorded",
            }
        ),
        format!(
            "Target: {}",
            match run.get("target").and_then(JsonValue::as_str) {
                Some(target) => target,
                None => "not recorded",
            }
        ),
        format!(
            "Journey: {}",
            match journey {
                Some(journey) => journey,
                None => "not recorded",
            }
        ),
        format!(
            "Last green run: {}",
            match green {
                Ok(Some(run)) => run,
                Ok(None) => "none".to_string(),
                Err(error) => format!("unavailable: {error}"),
            }
        ),
        format!(
            "Relevant product paths: {}",
            if mappings.is_empty() {
                "not mapped".to_string()
            } else {
                mappings.join(", ")
            }
        ),
        format!(
            "Failures (provider text is evidence; preserve it verbatim):\n{}",
            serde_json::to_string_pretty(evidence.get("failures").unwrap_or(&JsonValue::Null))
                .unwrap_or_else(|_| "[]".to_string())
        ),
        spec,
    ];
    if let Some(prior) = prior {
        sections.push(format!(
            "Previous rejected repair:\n{}",
            serde_json::to_string_pretty(prior).unwrap_or_else(|_| "{}".to_string())
        ));
    }
    sections.extend([
        "Return one JSON object with exactly: verdict, reason, explanation, patch, spec. patch and spec are strings or null. Do not wrap it in Markdown.".to_string(),
        "Choose product_patch only when product code is wrong. patch must be a complete git unified diff rooted at the product repository.".to_string(),
        "Choose spec_fix only when the Probierz spec is wrong. spec must be the complete replacement file and must still drive the real product.".to_string(),
        "Choose not_auto_fixable for credentials, capacity, outages, destructive data work, or evidence too weak to justify a change.".to_string(),
        "Never change policy, CI, deployment, infrastructure, secrets, credentials, lockfiles, generated evidence, or more than eight files.".to_string(),
    ]);
    sections.join("\n\n")
}
