//! Figma parity of @wisent-ai/components: does the package's own render of a
//! component look like the component?
//!
//! The candidate is what the package draws from its checked-in tree; the
//! reference is the SVG Figma exported for the same node, on a page that
//! carries none of the package's code, so a package defect cannot move it.
//! Both are captured by one Weles browser at the node's size, compared pixel
//! by pixel (`compare`), and each component leaves candidate, reference,
//! parity.json and, over its limit, a difference mask. A component whose
//! export has not landed is reported skipped by name, never passed.

use serde_json::json;

use crate::specs::web::weles::{Page, Weles};
use crate::specs::*;

mod compare;
mod constants;
mod server;

/// The thresholds must say what they raise and why: an override that is not
/// a component, not above the default, or has no reason is refused.
fn check_thresholds(plan: &Value) -> Result<(u64, f64), String> {
    let tolerance = plan["channelTolerance"]
        .as_u64()
        .filter(|tolerance| *tolerance <= u64::from(u8::MAX))
        .ok_or("parity-thresholds.json channelTolerance must be 0…255")?;
    let default = plan["defaultMaxDifferingPixelRatio"]
        .as_f64()
        .filter(|ratio| *ratio > 0.0 && *ratio <= 1.0)
        .ok_or("parity-thresholds.json defaultMaxDifferingPixelRatio must be a share above 0")?;
    let components = plan["components"]
        .as_array()
        .ok_or("the parity plan lists no components")?;
    for (key, entry) in plan["overrides"].as_object().into_iter().flatten() {
        if !components
            .iter()
            .any(|component| component["key"] == key.as_str())
        {
            return Err(format!(
                "parity-thresholds.json raises {key}, which is not a component"
            ));
        }
        let ratio = entry["maxDifferingPixelRatio"]
            .as_f64()
            .filter(|ratio| *ratio > default && *ratio <= 1.0);
        if ratio.is_none() {
            return Err(format!(
                "{key} needs a maxDifferingPixelRatio above the default and at most 1"
            ));
        }
        if entry["reason"]
            .as_str()
            .is_none_or(|reason| reason.trim().is_empty())
        {
            return Err(format!("{key} raises the threshold without a reason"));
        }
    }
    Ok((tolerance, default))
}

/// Open `address` at the node's size, wait for `selector` and for fonts and
/// images to finish loading, and keep a PNG of the viewport at `path`.
fn capture(
    page: &Page,
    address: &str,
    selector: &str,
    size: (u32, u32),
    path: &Path,
) -> Result<(), String> {
    page.viewport(size.0, size.1)?;
    page.goto(address)?;
    page.wait(selector, "visible")?;
    page.evaluate("(async () => { await document.fonts.ready; await Promise.all([...document.images].filter((image) => !image.complete).map((image) => image.decode())); return true; })()")?;
    page.screenshot_png(path)
}

/// wisent-components-figma-parity: every literal Figma component the package ships matches its Figma export within parity-thresholds.json.
pub(crate) fn figma_parity(context: &Context) -> Result<(), String> {
    let server = server::Server::start(context)?;
    let plan_url = server
        .base
        .join("/parity/plan")
        .map_err(|error| error.to_string())?;
    let plan: Value = ureq::get(plan_url.as_str())
        .call()
        .map_err(|error| format!("{plan_url}: {error}"))?
        .into_json()
        .map_err(|error| format!("{plan_url} is not JSON: {error}"))?;
    let (tolerance, default) = check_thresholds(&plan)?;
    let weles = Weles::start(context)?;
    let candidate_page = weles.page(Some(server.base.clone()))?;
    let reference_page = weles.page(Some(server.base.clone()))?;
    let (mut results, mut failures, mut skipped) = (Vec::new(), Vec::new(), Vec::new());
    for component in plan["components"].as_array().into_iter().flatten() {
        let key = component["key"]
            .as_str()
            .ok_or("a parity component has no key")?;
        let (Some(render), Some(width), Some(height)) = (
            component["render"].as_str(),
            component["width"].as_u64(),
            component["height"].as_u64(),
        ) else {
            skipped.push(key.to_string());
            continue;
        };
        let size = (
            u32::try_from(width).map_err(|e| e.to_string())?,
            u32::try_from(height).map_err(|e| e.to_string())?,
        );
        let override_entry = &plan["overrides"][key];
        let limit = override_entry["maxDifferingPixelRatio"]
            .as_f64()
            .unwrap_or(default);
        let directory = context
            .artifacts
            .join("figma-parity")
            .join(key.replace([':', '/'], "-"));
        fs::create_dir_all(&directory)
            .map_err(|error| format!("{}: {error}", directory.display()))?;
        let (candidate, reference) = (
            directory.join("candidate.png"),
            directory.join("reference.png"),
        );
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("component", key)
            .finish();
        capture(
            &candidate_page,
            &format!("/?{query}"),
            "#candidate",
            size,
            &candidate,
        )?;
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("asset", render)
            .append_pair("width", &width.to_string())
            .append_pair("height", &height.to_string())
            .finish();
        capture(
            &reference_page,
            &format!("/reference?{query}"),
            "#reference",
            size,
            &reference,
        )?;
        let mut measured =
            compare::compare(&reference_page, &candidate, &reference, tolerance, false)?;
        let ratio = measured["ratio"]
            .as_f64()
            .ok_or("the comparison answered no ratio")?;
        if ratio > limit {
            measured = compare::compare(&reference_page, &candidate, &reference, tolerance, true)?;
            let mask = measured["mask"]
                .as_str()
                .ok_or("the comparison answered no mask")?;
            let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, mask)
                .map_err(|e| e.to_string())?;
            let difference = directory.join("difference.png");
            fs::write(&difference, bytes)
                .map_err(|error| format!("{}: {error}", difference.display()))?;
            context.media_typed("screenshot", difference, "image/png");
            failures.push(format!(
                "{key} differs from its Figma export in {} of {} pixels ({:.3} %); the limit is {:.3} %{}",
                measured["differing"], measured["total"], ratio * 100.0, limit * 100.0,
                override_entry["reason"].as_str().map(|reason| format!(" ({reason})")).unwrap_or_default()
            ));
        }
        let verdict = json!({ "componentKey": key, "name": component["name"], "render": render, "box": { "width": width, "height": height }, "candidate": measured["candidate"], "reference": measured["reference"], "channelTolerance": tolerance, "differingPixels": measured["differing"], "comparedPixels": measured["total"], "differingPixelRatio": ratio, "maxDifferingPixelRatio": limit, "thresholdReason": override_entry["reason"], "within": ratio <= limit });
        fs::write(directory.join("parity.json"), verdict.to_string())
            .map_err(|error| error.to_string())?;
        context.media_typed("screenshot", candidate, "image/png");
        context.media_typed("screenshot", reference, "image/png");
        results.push(verdict);
    }
    drop(weles);
    drop(server);
    let summary = context.artifacts.join("figma-parity.json");
    let text = json!({ "compared": results.len(), "skipped": skipped, "failed": failures, "components": results }).to_string();
    fs::write(&summary, text).map_err(|error| format!("{}: {error}", summary.display()))?;
    context.media_typed("trace", summary, "application/json");
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}
