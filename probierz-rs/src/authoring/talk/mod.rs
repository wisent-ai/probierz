//! Evaluating a rendered talk against the documents it presents.
//!
//! | part | what it owns |
//! |---|---|
//! | `given` | the slides in order, the speaker script, the source documents, the router, each read once |
//! | `rubric` | the criteria a talk is judged by |
//! | `grade` | the model request, its single tool call, and the unmet criteria |
//! | `report` | the identity of what was judged, the verdict, and the written report |
//!
//! Every part opens with `use super::*;`, so the list below is this
//! evaluation's single import list.

pub(crate) use serde_json::json;

pub(crate) use crate::authoring::*;

mod given;
mod grade;
mod report;
mod rubric;

pub(crate) use given::*;
pub(crate) use grade::*;
pub(crate) use report::*;
pub(crate) use rubric::*;

/// Evaluate a rendered talk and write the report.
///
/// A vision model is shown every slide picture in order, the speaker script
/// and the full text of every source document, and judges each rubric
/// criterion met or not with its evidence. The talk passes only when every
/// criterion is met and the model names no blocker.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_talk(
    harness: &Path,
    slides: &Path,
    script: &Path,
    sources: &[PathBuf],
    rubric_file: Option<&Path>,
    output: Option<&Path>,
    model: Option<&str>,
    router_url: Option<&str>,
    router_bearer: Option<&str>,
    agent_id: Option<&str>,
    agent_secret: Option<&str>,
) -> Result<JsonValue, Failure> {
    let talk = Talk::resolve(slides, script, sources)?;
    let rubric = talk_rubric(rubric_file)?;
    let router = TalkRouter::resolve(model, router_url, router_bearer, agent_id, agent_secret)?;
    let report = report_path(harness, &talk, output)?;
    let identity = identity_document(&talk, &rubric);
    let graded = grade_talk(&talk, &rubric, &router)?;
    let value = talk_report(identity, graded);
    write_talk_report(&report, &value)?;
    Ok(value)
}
