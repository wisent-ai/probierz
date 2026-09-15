//! The model half: two independent graders, a third adjudicating when
//! they disagree, and the merged view their scores produce.
//!
//! Two graders that resolve to the same model would agree for the wrong
//! reason, so that is an error rather than a pass. Without an
//! adjudicator the merged score is the lower of the two and only the
//! issues both raised count as blockers — a single grader cannot fail
//! the verdict on its own.

use super::*;

/// Score delta above which an adjudicator is required, when the policy
/// does not state its own.
const DEFAULT_ADJUDICATION_DELTA: f64 = 0.2;

/// Scores are reported to four decimal places, which is the resolution
/// the report and its signature carry.
const SCORE_SCALE: f64 = 10_000.0;

pub(crate) fn round_score(value: f64) -> f64 {
    (value * SCORE_SCALE).round() / SCORE_SCALE
}

/// The model view of the site, and what produced it.
pub(crate) struct Graded {
    pub(crate) model_evaluation: JsonValue,
}

pub(crate) fn grade_with_models(
    harness: &Path,
    app_id: &str,
    contract: &Contract,
    models: &ModelSettings,
    crawled: &Crawled,
    adjudicator: Option<&str>,
) -> Result<Graded, Failure> {
    let primary_grade = invoke(models, &models.primary, contract, crawled, None)?;
    let secondary_grade = invoke(models, &models.secondary, contract, crawled, None)?;

    if primary_grade["modelReturned"].is_null() || secondary_grade["modelReturned"].is_null() {
        return Err(Failure::config(
            "seo-evaluate.model",
            "SEO graders did not identify the model versions that produced their evaluations",
        ));
    }
    if primary_grade["modelReturned"] == secondary_grade["modelReturned"] {
        return Err(Failure::config(
            "seo-evaluate.model",
            format!(
                "SEO graders resolved to the same model {}",
                primary_grade["modelReturned"].as_str().unwrap_or_default()
            ),
        ));
    }

    let names = contract.model_dimension_names();
    let score_delta = names
        .iter()
        .map(|name| {
            (dimension_score(&primary_grade, name) - dimension_score(&secondary_grade, name)).abs()
        })
        .fold(0.0_f64, f64::max);
    let primary_codes = blocking_codes(&primary_grade);
    let secondary_codes = blocking_codes(&secondary_grade);
    let blocker_mismatch = primary_codes != secondary_codes;

    let delta = contract
        .policy
        .pointer("/model/adjudicationDelta")
        .and_then(JsonValue::as_f64)
        .unwrap_or(DEFAULT_ADJUDICATION_DELTA);
    let divergence_required = score_delta > delta || blocker_mismatch;

    let adjudicator_grade = if divergence_required {
        Some(adjudicate(
            harness,
            app_id,
            contract,
            models,
            crawled,
            adjudicator,
            &primary_grade,
            &secondary_grade,
        )?)
    } else {
        None
    };

    let model_dimensions = merge_dimensions(
        &names,
        &primary_grade,
        &secondary_grade,
        adjudicator_grade.as_ref(),
    );
    let model_blockers = agreed_blockers(
        &primary_grade,
        &secondary_codes,
        adjudicator_grade.as_ref(),
    );
    let model_recommendations = recommendations(
        &primary_grade,
        &secondary_grade,
        adjudicator_grade.as_ref(),
    );

    Ok(Graded {
        model_evaluation: json!({
            "dimensions": model_dimensions,
            "blockers": model_blockers,
            "recommendations": model_recommendations,
            "divergence": {
                "required": divergence_required,
                "scoreDelta": round_score(score_delta),
                "blockerMismatch": blocker_mismatch
            },
            "graders": {
                "primary": primary_grade,
                "secondary": secondary_grade,
                "adjudicator": adjudicator_grade
            }
        }),
    })
}

fn invoke(
    models: &ModelSettings,
    model: &str,
    contract: &Contract,
    crawled: &Crawled,
    task: Option<&JsonValue>,
) -> Result<JsonValue, Failure> {
    invoke_seo_model(
        model,
        &models.router_url,
        &models.token,
        &models.agent_id,
        &models.agent_secret,
        &contract.policy,
        &contract.brief,
        &crawled.evidence,
        &crawled.deterministic,
        task,
    )
}

/// The third model, which must differ from both graders, reading their
/// two evaluations against the approved brief and the same evidence.
#[allow(clippy::too_many_arguments)]
fn adjudicate(
    _harness: &Path,
    _app_id: &str,
    contract: &Contract,
    models: &ModelSettings,
    crawled: &Crawled,
    adjudicator: Option<&str>,
    primary_grade: &JsonValue,
    secondary_grade: &JsonValue,
) -> Result<JsonValue, Failure> {
    let model = contract.setting(adjudicator, "PROBIERZ_SEO_ADJUDICATOR_MODEL")?;
    if model == models.primary || model == models.secondary {
        return Err(Failure::config(
            "seo-evaluate",
            "SEO adjudicator model ID must differ from both graders",
        ));
    }
    invoke(
        models,
        &model,
        contract,
        crawled,
        Some(&json!({
            "task": "Adjudicate the two evaluations against the original approved brief and page evidence.",
            "originalEvidence": crawled.evidence,
            "primary": primary_grade["evaluation"],
            "secondary": secondary_grade["evaluation"]
        })),
    )
}

fn dimension_score(grade: &JsonValue, name: &str) -> f64 {
    grade["evaluation"]["dimensions"][name]["score"]
        .as_f64()
        .unwrap_or(0.0)
}

fn blocking_codes(grade: &JsonValue) -> BTreeSet<String> {
    grade["evaluation"]["blocking_issues"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["code"].as_str().map(str::to_string))
        .collect()
}

/// Per dimension: the adjudicator's score when there is one, otherwise
/// the lower of the two graders'. Evidence and issues from every grader
/// are kept, deduplicated, so the report shows why.
fn merge_dimensions(
    names: &[String],
    primary_grade: &JsonValue,
    secondary_grade: &JsonValue,
    adjudicator_grade: Option<&JsonValue>,
) -> Map<String, JsonValue> {
    let mut dimensions = Map::new();
    for name in names {
        let graders: Vec<&JsonValue> = [primary_grade, secondary_grade]
            .into_iter()
            .chain(adjudicator_grade)
            .collect();
        let score = adjudicator_grade.map_or_else(
            || {
                dimension_score(primary_grade, name).min(dimension_score(secondary_grade, name))
            },
            |grade| dimension_score(grade, name),
        );
        let mut seen = BTreeSet::new();
        let issues: Vec<String> = graders
            .iter()
            .flat_map(|grade| {
                grade["evaluation"]["dimensions"][name]["issues"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .filter_map(JsonValue::as_str)
            .filter(|issue| seen.insert((*issue).to_string()))
            .map(str::to_string)
            .collect();
        dimensions.insert(
            name.clone(),
            json!({
                "score": round_score(score),
                "evidence": graders.iter().flat_map(|grade| {
                    grade["evaluation"]["dimensions"][name]["evidence"].as_array().into_iter().flatten()
                }).cloned().collect::<Vec<_>>(),
                "issues": issues,
                "graderScores": graders.iter().map(|grade| (
                    grade["modelRequested"].as_str().unwrap_or_default().to_string(),
                    grade["evaluation"]["dimensions"][name]["score"].clone()
                )).collect::<Map<_, _>>()
            }),
        );
    }
    dimensions
}

/// The adjudicator's blocking issues when it ran; otherwise only the
/// issues both graders raised.
fn agreed_blockers(
    primary_grade: &JsonValue,
    secondary_codes: &BTreeSet<String>,
    adjudicator_grade: Option<&JsonValue>,
) -> Vec<JsonValue> {
    let agreed: Vec<JsonValue> = match adjudicator_grade {
        Some(grade) => grade["evaluation"]["blocking_issues"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
        None => primary_grade["evaluation"]["blocking_issues"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| secondary_codes.contains(item["code"].as_str().unwrap_or_default()))
            .cloned()
            .collect(),
    };
    agreed
        .into_iter()
        .map(|mut item| {
            if let Some(object) = item.as_object_mut() {
                object.insert("source".to_string(), json!("model"));
            }
            item
        })
        .collect()
}

fn recommendations(
    primary_grade: &JsonValue,
    secondary_grade: &JsonValue,
    adjudicator_grade: Option<&JsonValue>,
) -> Vec<JsonValue> {
    [primary_grade, secondary_grade]
        .into_iter()
        .chain(adjudicator_grade)
        .flat_map(|grade| {
            grade["evaluation"]["recommendations"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .cloned()
        .collect()
}
