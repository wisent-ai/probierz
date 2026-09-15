//! Evaluating a candidate scientific figure against its reference.
//!
//! | part | what it owns |
//! |---|---|
//! | `inputs` | the two files, the rubric, the router, the output paths |
//! | `grade` | the model request, its single tool call, and the scores |
//! | `report` | the identity document, the renders, and the written report |
//!
//! Every part opens with `use super::*;`, so the list below is this
//! evaluation's single import list.

pub(crate) use serde_json::json;

pub(crate) use crate::authoring::*;

mod grade;
mod inputs;
mod report;

pub(crate) use grade::*;
pub(crate) use inputs::*;
pub(crate) use report::*;

/// Evaluate a candidate figure against a reference and write the
/// report.
///
/// Both files are rendered to PNG first, their geometry is measured
/// without a model, and a vision model then scores every rubric
/// dimension against both images. A candidate that cannot be rendered
/// is a failed verdict with the renderer's own error, not an error from
/// this command.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_figure(
    harness: &Path,
    reference: &Path,
    candidate: &Path,
    rubric_file: Option<&Path>,
    output: Option<&Path>,
    model: Option<&str>,
    router_url: Option<&str>,
    tex_preamble: Option<&Path>,
    router_bearer: Option<&str>,
    agent_id: Option<&str>,
    agent_secret: Option<&str>,
) -> Result<JsonValue, Failure> {
    let pair = FigurePair::resolve(reference, candidate)?;
    let rubric = figure_rubric(rubric_file)?;
    let router = Router::resolve(harness, model, router_url, router_bearer, agent_id, agent_secret)?;
    let destination = Destination::resolve(harness, &pair, output)?;

    let work = work_directory()?;
    let evaluated = evaluate_in(&work, &pair, &rubric, &router, &destination, tex_preamble);
    let _ = fs::remove_dir_all(&work);

    let value = evaluated?;
    write_report(&destination.report, &value)?;
    Ok(value)
}

/// The evaluation itself, inside the working directory the renders go
/// into, so the caller can remove it whichever way this ends.
fn evaluate_in(
    work: &Path,
    pair: &FigurePair,
    rubric: &JsonValue,
    router: &Router,
    destination: &Destination,
    tex_preamble: Option<&Path>,
) -> Result<JsonValue, Failure> {
    let reference_render = render(&pair.reference, work, "reference", tex_preamble)?;
    let reference_geometry = geometry(&reference_render)?;
    let identity = identity_document(pair, rubric, tex_preamble)?;

    let candidate_render = match render_figure(&pair.candidate, work, "candidate", tex_preamble) {
        Ok(file) => file,
        Err(detail) => {
            return unrenderable_candidate_report(
                identity,
                destination,
                &reference_render,
                &reference_geometry,
                &detail,
            )
        }
    };
    let candidate_geometry = geometry(&candidate_render)?;
    let deterministic = deterministic_figure(&reference_geometry, &candidate_geometry);

    let graded = grade_figure(
        rubric,
        router,
        &deterministic,
        &reference_render,
        &reference_geometry,
        &candidate_render,
        &candidate_geometry,
    )?;

    graded_report(
        identity,
        destination,
        Renders {
            reference: &reference_render,
            reference_geometry: &reference_geometry,
            candidate: &candidate_render,
            candidate_geometry: &candidate_geometry,
        },
        deterministic,
        graded,
    )
}
