//! One component measured: its candidate and reference captured at the node's
//! size, compared, and its parity.json written. The caller keeps going when
//! this fails, so one broken component never hides the verdicts after it.

use serde_json::json;

use super::{capture, compare};
use crate::specs::web::weles::Page;
use crate::specs::*;

pub(super) struct Target<'a> {
    pub(super) key: &'a str,
    pub(super) name: &'a Value,
    pub(super) render: &'a str,
    pub(super) width: u64,
    pub(super) height: u64,
    pub(super) limit: f64,
    pub(super) reason: &'a Value,
    pub(super) tolerance: u64,
}

/// The component's verdict, and the failure sentence when it is over its limit.
pub(super) fn measure(
    context: &Context,
    candidate_page: &Page,
    reference_page: &Page,
    target: &Target,
) -> Result<(Value, Option<String>), String> {
    let size = (
        u32::try_from(target.width).map_err(|e| e.to_string())?,
        u32::try_from(target.height).map_err(|e| e.to_string())?,
    );
    let directory = context
        .artifacts
        .join("figma-parity")
        .join(target.key.replace([':', '/'], "-"));
    fs::create_dir_all(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let (candidate, reference) = (
        directory.join("candidate.png"),
        directory.join("reference.png"),
    );
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("component", target.key)
        .finish();
    capture(
        candidate_page,
        &format!("/?{query}"),
        "#candidate",
        size,
        &candidate,
    )?;
    context.media_typed("screenshot", candidate.clone(), "image/png");
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("asset", target.render)
        .append_pair("width", &target.width.to_string())
        .append_pair("height", &target.height.to_string())
        .finish();
    capture(
        reference_page,
        &format!("/reference?{query}"),
        "#reference",
        size,
        &reference,
    )?;
    context.media_typed("screenshot", reference.clone(), "image/png");
    let mut measured = compare::compare(
        reference_page,
        &candidate,
        &reference,
        target.tolerance,
        false,
    )?;
    let ratio = measured["ratio"]
        .as_f64()
        .ok_or("the comparison answered no ratio")?;
    let mut failure = None;
    if ratio > target.limit {
        measured = compare::compare(
            reference_page,
            &candidate,
            &reference,
            target.tolerance,
            true,
        )?;
        let mask = measured["mask"]
            .as_str()
            .ok_or("the comparison answered no mask")?;
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, mask)
            .map_err(|e| e.to_string())?;
        let difference = directory.join("difference.png");
        fs::write(&difference, bytes)
            .map_err(|error| format!("{}: {error}", difference.display()))?;
        context.media_typed("screenshot", difference, "image/png");
        failure =
            Some(format!(
            "{} differs from its Figma export in {} of {} pixels ({:.3} %); the limit is {:.3} %{}",
            target.key,
            measured["differing"],
            measured["total"],
            ratio * 100.0,
            target.limit * 100.0,
            target.reason.as_str().map(|reason| format!(" ({reason})")).unwrap_or_default()
        ));
    }
    let verdict = json!({
        "componentKey": target.key, "name": target.name, "render": target.render,
        "box": { "width": target.width, "height": target.height },
        "candidate": measured["candidate"], "reference": measured["reference"],
        "channelTolerance": target.tolerance, "differingPixels": measured["differing"],
        "comparedPixels": measured["total"], "differingPixelRatio": ratio,
        "maxDifferingPixelRatio": target.limit, "thresholdReason": target.reason,
        "within": ratio <= target.limit,
    });
    fs::write(directory.join("parity.json"), verdict.to_string())
        .map_err(|error| error.to_string())?;
    Ok((verdict, failure))
}
