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
mod measure;
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
        let override_entry = &plan["overrides"][key];
        let target = measure::Target {
            key,
            name: &component["name"],
            render,
            width,
            height,
            limit: override_entry["maxDifferingPixelRatio"]
                .as_f64()
                .unwrap_or(default),
            reason: &override_entry["reason"],
            tolerance,
        };
        // Every component gets its verdict: an error here is that component's
        // failure, recorded, and the next component is still measured.
        match measure::measure(context, &candidate_page, &reference_page, &target) {
            Ok((verdict, failure)) => {
                failures.extend(failure);
                results.push(verdict);
            }
            Err(error) => {
                failures.push(format!("{key} could not be measured: {error}"));
                results.push(
                    json!({ "componentKey": key, "name": component["name"], "error": error }),
                );
            }
        }
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
