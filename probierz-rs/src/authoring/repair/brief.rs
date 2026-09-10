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
    let root = repository
        .and_then(|value| value.get("root"))
        .and_then(YamlValue::as_str)
        .unwrap_or("unknown");
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
    let spec = recorded_spec_path(harness, run)
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|value| value.chars().take(12_000).collect::<String>());
    let green = crate::status::run_history_value(
        harness,
        app_id,
        run.get("target").and_then(JsonValue::as_str),
        100,
    )
    .ok()
    .and_then(|history| {
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
            run.get("runId")
                .and_then(JsonValue::as_str)
                .unwrap_or_default()
        ),
        format!(
            "Target: {}",
            run.get("target")
                .and_then(JsonValue::as_str)
                .unwrap_or_default()
        ),
        format!("Journey: {}", journey.unwrap_or("unknown")),
        format!("Last green run: {}", green.as_deref().unwrap_or("none")),
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
        spec.map(|value| format!("Current Probierz spec:\n{value}"))
            .unwrap_or_else(|| "No exact spec file was found.".to_string()),
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

