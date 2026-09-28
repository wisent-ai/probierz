//! The landing page release evaluation: does the page that is about to ship
//! still do what its approved brief promised?
//!
//! The run reads the brief and the rubric, looks at the page at desktop,
//! tablet and mobile size through Weles, checks the promised action really
//! goes somewhere, asks the routed model to grade what it sees, and writes a
//! report a person can read. `inputs` does the reading, `capture` the
//! looking, `verdict::judge` the asking and `verdict` the write-up.

use serde_json::{json, Map};

use constants::{DESKTOP, MOBILE, PROOF_SCROLL_SHARE, TABLET, UNSEEN_SCORE};

use crate::specs::web::weles::Weles;
use crate::specs::*;

mod capture;
mod constants;
mod inputs;
mod verdict;

/// landing-page-release-evaluation: desktop, tablet, and mobile evidence satisfy the release rubric.
pub(crate) fn release_evaluation(context: &Context) -> Result<(), String> {
    let url = inputs::target(context)?;
    let (brief_path, brief) = inputs::brief(context)?;
    let rubric = inputs::rubric(context)?;
    let label = brief["primaryAction"]["label"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    let weles = Weles::start(context)?;
    let mut audits = Map::new();
    let mut images: Vec<(String, PathBuf)> = Vec::new();
    let mut conversion = None;
    let share = (PROOF_SCROLL_SHARE * 100.0).round();
    for (profile, title, (width, height)) in [
        ("desktop", "Desktop", DESKTOP),
        ("tablet", "Tablet", TABLET),
        ("mobile", "Mobile", MOBILE),
    ] {
        let captured = capture::viewport(context, &weles, &url, profile, &label)?;
        images.push((
            format!("{title} first viewport, {width} by {height} CSS pixels"),
            captured.hero.clone(),
        ));
        images.push((
            format!("{title} proof section near {share} percent of the scroll range"),
            captured.proof.clone(),
        ));
        audits.insert(profile.to_string(), captured.audit.clone());
        if conversion.is_none() {
            conversion = Some(capture::primary_action(
                &captured.page,
                &brief["primaryAction"],
            )?);
        }
    }
    drop(weles);
    let audits = Value::Object(audits);
    let conversion = conversion.ok_or("no viewport was captured")?;

    let (model, failure) = match verdict::judge::evaluate(
        context, &rubric, &brief, &audits, &images,
    ) {
        Ok(model) => (model, None),
        Err(failure) => {
            // The captures, audits and conversion evidence are already
            // collected: an outage of the vision route keeps them in the
            // report and still fails, because no grader saw the page and so
            // no dimension earned a score.
            let unseen: Map<String, Value> = rubric["dimensions"]
                .as_object()
                .into_iter()
                .flat_map(Map::keys)
                .map(|name| (name.clone(), json!({ "score": UNSEEN_SCORE, "evidence": ["model evaluation unavailable"], "issues": [] })))
                .collect();
            let evaluation = json!({
                "summary": format!("model evaluation unavailable: {failure}"),
                "dimensions": unseen,
                "blocking_issues": [],
                "recommendations": [],
            });
            let model = json!({ "evaluation": evaluation, "routerModel": context.optional("PROBIERZ_LANDING_VISION_MODEL"), "usage": null });
            (model, Some(failure))
        }
    };
    let mut report = verdict::report(
        &rubric,
        &brief_path,
        &brief,
        &audits,
        conversion,
        &model,
        &images,
    );
    if let Some(failure) = &failure {
        report["modelEvaluationFailed"] = json!(failure);
        if let Some(blockers) = report["blockers"].as_array_mut() {
            blockers.push(json!({ "code": "model_evaluation_unavailable", "evidence": failure, "source": "model" }));
        }
        report["pass"] = json!(false);
    }

    let report_path = context.artifacts.join("landing-page-evaluation.json");
    let text = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    fs::write(&report_path, format!("{text}\n"))
        .map_err(|error| format!("{}: {error}", report_path.display()))?;
    for (_, path) in &images {
        context.media_typed("screenshot", path.clone(), "image/jpeg");
    }
    context.media_typed("trace", report_path.clone(), "application/json");

    if report["pass"] == true {
        return Ok(());
    }
    let summary = json!({
        "report": report_path,
        "overall": report["overall"],
        "required": rubric["overallMinimum"],
        "blockers": report["blockers"],
        "dimensions": report["dimensions"],
    });
    Err(serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?)
}
