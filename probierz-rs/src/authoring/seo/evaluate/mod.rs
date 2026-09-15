//! Evaluating a site against its declared SEO contract, in the order
//! the evaluation happens.
//!
//! | part | what it owns |
//! |---|---|
//! | `inputs` | the mode, the base URL, the policy and brief, the model settings |
//! | `crawl` | fetching every declared route twice and the deterministic checks |
//! | `grade` | the two graders, the adjudicator, and the merged model view |
//! | `verdict` | merging both views, the thresholds, the signed report |
//!
//! Every part opens with `use super::*;`, so the list below is this
//! evaluation's single import list.

pub(crate) use serde_json::json;

pub(crate) use crate::authoring::*;

mod crawl;
mod grade;
mod inputs;
mod verdict;

pub(crate) use crawl::*;
pub(crate) use grade::*;
pub(crate) use inputs::*;
pub(crate) use verdict::*;

/// Evaluate one application's public pages against its declared SEO
/// policy and approved brief, and write the signed report.
///
/// The evaluation is deterministic first — every declared route is
/// fetched as ordinary Chrome and as Googlebot Smartphone — then
/// graded by two independent models, with a third adjudicating when
/// they disagree. A blocker from either half fails the verdict.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_seo(
    harness: &Path,
    app_id: &str,
    base_url: &str,
    policy_file: Option<&Path>,
    brief_file: Option<&Path>,
    mode: &str,
    output: Option<&Path>,
    production_file: Option<&Path>,
    primary: Option<&str>,
    secondary: Option<&str>,
    adjudicator: Option<&str>,
    router_url: Option<&str>,
    agent_id: Option<&str>,
    private_key_file: Option<&Path>,
    router_bearer: Option<&str>,
    agent_secret: Option<&str>,
    private_key: Option<&str>,
) -> Result<JsonValue, Failure> {
    let contract = Contract::load(harness, app_id, base_url, mode, policy_file, brief_file)?;
    let models = ModelSettings::resolve(
        harness,
        app_id,
        &contract,
        primary,
        secondary,
        router_url,
        agent_id,
        router_bearer,
        agent_secret,
    )?;

    let crawled = crawl_routes(&contract)?;
    let graded = grade_with_models(harness, app_id, &contract, &models, &crawled, adjudicator)?;

    write_report(
        harness,
        app_id,
        &contract,
        &crawled,
        &graded,
        output,
        production_file,
        private_key_file,
        private_key,
    )
}
