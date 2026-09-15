//! The report: what was evaluated, what the renders were, and the
//! verdict — written once, next to the two PNGs it refers to.

use super::*;

/// Schema version and kind of the report this writes.
const REPORT_SCHEMA_VERSION: u64 = 1;
const REPORT_KIND: &str = "probierz-figure-evaluation";

/// How much of a renderer failure is quoted as evidence. Long enough
/// for a LaTeX error with its context, short enough to read.
const RENDER_FAILURE_EXCERPT: usize = 4_000;

/// The two renders and what was measured from them.
pub(crate) struct Renders<'a> {
    pub(crate) reference: &'a Path,
    pub(crate) reference_geometry: &'a JsonValue,
    pub(crate) candidate: &'a Path,
    pub(crate) candidate_geometry: &'a JsonValue,
}

/// What was evaluated: both inputs by digest, the rubric, and the
/// renderer versions that produced the images.
pub(crate) fn identity_document(
    pair: &FigurePair,
    rubric: &JsonValue,
    tex_preamble: Option<&Path>,
) -> Result<JsonValue, Failure> {
    let image_magick = tool_version("magick", "-version")?;
    let pdf_latex = if pair.uses_latex() {
        JsonValue::String(tool_version("pdflatex", "--version")?)
    } else {
        JsonValue::Null
    };
    Ok(json!({
        "schemaVersion": REPORT_SCHEMA_VERSION,
        "kind": REPORT_KIND,
        "createdAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "inputs": {
            "reference": figure_input(&pair.reference)?,
            "candidate": figure_input(&pair.candidate)?
        },
        "rubric": rubric,
        "renderer": {
            "imageMagick": image_magick,
            "pdfLaTeX": pdf_latex,
            "texPreamble": tex_preamble
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| "built-in".to_string())
        }
    }))
}

/// The report for a candidate that would not render: the reference is
/// still published, the verdict fails with nothing scored, and the
/// renderer's own error is the evidence.
pub(crate) fn unrenderable_candidate_report(
    identity: JsonValue,
    destination: &Destination,
    reference_render: &Path,
    reference_geometry: &JsonValue,
    detail: &str,
) -> Result<JsonValue, Failure> {
    fs::copy(reference_render, &destination.reference_output)?;
    let mut report = identity;
    let object = object_of(&mut report)?;
    object.insert(
        "renders".to_string(),
        json!({
            "reference": render_record(&destination.reference_output, reference_geometry)?,
            "candidate": JsonValue::Null
        }),
    );
    object.insert(
        "deterministic".to_string(),
        json!({ "blockers": [], "aspectRatioDrift": JsonValue::Null }),
    );
    object.insert("model".to_string(), JsonValue::Null);
    object.insert("evaluation".to_string(), json!({
        "summary": "The candidate could not be rendered, so no visual comparison was possible.",
        "dimensions": {}, "blockers": [], "fidelityLosses": [],
        "recommendations": [{
            "priority": "critical",
            "action": "Fix the renderer error reported below and return a candidate that builds."
        }]
    }));
    object.insert(
        "verdict".to_string(),
        json!({
            "pass": false,
            "overall": 0,
            "blockers": [{
                "code": "candidate_render_failed",
                "artifact": "candidate",
                "evidence": detail.chars().take(RENDER_FAILURE_EXCERPT).collect::<String>()
            }]
        }),
    );
    object.insert(
        "reportPath".to_string(),
        json!(destination.report.to_string_lossy()),
    );
    Ok(report)
}

/// The report for a graded pair: both renders published, both halves of
/// the verdict kept, and every blocker from either.
pub(crate) fn graded_report(
    identity: JsonValue,
    destination: &Destination,
    renders: Renders<'_>,
    deterministic: JsonValue,
    graded: Graded,
) -> Result<JsonValue, Failure> {
    let mut blockers = deterministic["blockers"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    blockers.extend(
        graded.evaluation["blockers"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    blockers.extend(graded.threshold_blockers);

    fs::copy(renders.reference, &destination.reference_output)?;
    fs::copy(renders.candidate, &destination.candidate_output)?;

    let mut report = identity;
    let object = object_of(&mut report)?;
    object.insert(
        "renders".to_string(),
        json!({
            "reference": render_record(&destination.reference_output, renders.reference_geometry)?,
            "candidate": render_record(&destination.candidate_output, renders.candidate_geometry)?
        }),
    );
    object.insert("deterministic".to_string(), deterministic);
    object.insert("model".to_string(), graded.model);
    object.insert("evaluation".to_string(), graded.evaluation);
    object.insert(
        "verdict".to_string(),
        json!({
            "pass": blockers.is_empty(),
            "overall": graded.overall,
            "blockers": blockers
        }),
    );
    object.insert(
        "reportPath".to_string(),
        json!(destination.report.to_string_lossy()),
    );
    Ok(report)
}

/// Write the report exactly once: a path that already exists is an
/// error, not an overwrite.
pub(crate) fn write_report(report: &Path, value: &JsonValue) -> Result<(), Failure> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report)?;
    file.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// One published render: where it is, its digest, and its geometry.
fn render_record(file: &Path, geometry: &JsonValue) -> Result<JsonValue, Failure> {
    Ok(json!({
        "path": file.to_string_lossy(),
        "sha256": hex::encode(Sha256::digest(fs::read(file)?)),
        "width": geometry["width"],
        "height": geometry["height"],
        "aspectRatio": geometry["aspectRatio"],
        "contentBounds": geometry["contentBounds"],
        "margins": geometry["margins"]
    }))
}

/// One input figure: where it came from, its digest, and its type.
fn figure_input(file: &Path) -> Result<JsonValue, Failure> {
    Ok(json!({
        "path": file.to_string_lossy(),
        "sha256": hex::encode(Sha256::digest(fs::read(file)?)),
        "type": file.extension().and_then(OsStr::to_str).unwrap_or_default()
    }))
}

/// The first line of a renderer's version output, which is what the
/// report records as the renderer identity.
fn tool_version(program: &str, argument: &str) -> Result<String, Failure> {
    Ok(figure_process(program, &[argument.to_string()], None)
        .map_err(|detail| Failure::config("figure-evaluate.render", detail))?
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string())
}

fn object_of(value: &mut JsonValue) -> Result<&mut Map<String, JsonValue>, Failure> {
    value
        .as_object_mut()
        .ok_or_else(|| Failure::config("figure-evaluate", "figure identity is invalid"))
}
