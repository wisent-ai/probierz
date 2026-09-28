//! Looking at the page the way a visitor does.
//!
//! One viewport at a time: open it, let it settle, measure what is on screen
//! (overflow, headings, unnamed controls, layout shift, the console and
//! failed requests), keep two images as run evidence, and check that the
//! action the brief promises actually goes somewhere. Everything measured
//! here is deterministic; taste is asked of the model in `verdict`.

use serde_json::json;

use super::constants::{
    AUDIT, CAPTURE_QUALITY, DESKTOP, LAYOUT_SHIFT_OBSERVER, MOBILE, PROOF_SCROLL_SHARE, TABLET,
};
use crate::specs::web::weles::{role_exact, Page, Weles};
use crate::specs::*;

pub(super) struct Capture<'w> {
    pub(super) page: Page<'w>,
    pub(super) audit: Value,
    pub(super) hero: PathBuf,
    pub(super) proof: PathBuf,
}

const TWO_FRAMES: &str = "new Promise((settled) => requestAnimationFrame(() => requestAnimationFrame(() => settled(true))))";

/// Open `url` at one profile's viewport, measure it and keep its two images.
pub(super) fn viewport<'w>(
    context: &Context,
    weles: &'w Weles,
    url: &str,
    profile: &str,
    label: &str,
) -> Result<Capture<'w>, String> {
    let (width, height) = match profile {
        "mobile" => MOBILE,
        "tablet" => TABLET,
        _ => DESKTOP,
    };
    let page = weles.page(None)?;
    page.viewport(width, height)?;
    page.init_script(LAYOUT_SHIFT_OBSERVER)?;
    let opened = page.goto(url)?;
    page.wait_for("document.fonts ? document.fonts.status === 'loaded' : true")?;
    page.wait_for(TWO_FRAMES)?;
    let events = page.events(false)?;
    let arguments = json!({
        "label": label,
        "profileName": profile,
        "status": opened["status"],
        "capturedConsoleErrors": events["consoleErrors"],
        "capturedFailedRequests": events["failedRequests"],
    });
    let audit = page.evaluate(&format!("{AUDIT}({arguments})"))?;
    let hero = context.artifacts.join(format!("{profile}-hero.jpg"));
    let proof = context.artifacts.join(format!("{profile}-proof.jpg"));
    page.screenshot_jpeg(&hero, CAPTURE_QUALITY)?;
    page.evaluate(&format!(
        "window.scrollTo(window.scrollX, Math.round(Math.max(0, document.documentElement.scrollHeight - window.innerHeight) * {PROOF_SCROLL_SHARE}))"
    ))?;
    page.wait_for(TWO_FRAMES)?;
    page.screenshot_jpeg(&proof, CAPTURE_QUALITY)?;
    page.evaluate("window.scrollTo(window.scrollX, Number())")?;
    Ok(Capture {
        page,
        audit,
        hero,
        proof,
    })
}

/// Does the promised action lead where the brief says: the approved URL, the
/// form it belongs to, or a dialog naming the target?
pub(super) fn primary_action(page: &Page, action: &Value) -> Result<(bool, String), String> {
    let label = action["label"].as_str().unwrap_or_default();
    let target = action["target"].as_str().unwrap_or_default();
    let button = role_exact("button", label);
    let control = if page.query(&button, &[])?["count"]
        .as_u64()
        .unwrap_or_default()
        > 0
    {
        format!("{button} >> nth=0")
    } else {
        format!("{} >> nth=0", role_exact("link", label))
    };
    if page.query(&control, &[])?["count"]
        .as_u64()
        .unwrap_or_default()
        == 0
    {
        return Ok((false, format!("No accessible {} action", json!(label))));
    }
    match action["kind"].as_str() {
        Some("url") => {
            let link = page.query(
                &format!("{control} >> xpath=ancestor-or-self::a[1]"),
                &["href"],
            )?;
            let Some(href) = link["attributes"]["href"].as_str() else {
                return Ok((false, "Approved URL action is not a link".into()));
            };
            let resolved =
                page.evaluate(&format!("new URL({}, location.href).href", json!(href)))?;
            let resolved = resolved.as_str().unwrap_or(href);
            let pass = resolved == target || resolved.starts_with(target);
            Ok((
                pass,
                format!(
                    "{resolved} {} {target}",
                    if pass { "matches" } else { "does not match" }
                ),
            ))
        }
        Some("form") => {
            let form = page.query(&format!("{target} >> nth=0"), &[])?;
            if form["count"].as_u64().unwrap_or_default() == 0 {
                return Ok((false, format!("Form target {target} does not exist")));
            }
            if form["visible"].as_bool() != Some(true) {
                return Ok((false, format!("Form target {target} is not visible")));
            }
            let check = format!(
                "(() => {{ const target = document.querySelector({target}); const candidates = [...document.querySelectorAll('button,a,[role=\"button\"]')].filter((element) => element.innerText.replace(/\\s+/g, ' ').trim() === {label} || element.getAttribute('aria-label') === {label}); return candidates.some((element) => {{ const anchor = element.closest('a'); const controlled = element.getAttribute('aria-controls'); return Boolean(target) && (target.contains(element) || Boolean(anchor && anchor.hash && target.id && anchor.hash === `#${{target.id}}`) || element.closest('form') === target || Boolean(controlled && target.id === controlled)); }}); }})()",
                label = json!(label),
                target = json!(target)
            );
            let pass = page.evaluate(&check)?.as_bool() == Some(true);
            Ok((
                pass,
                format!(
                    "{label} {} form {target}",
                    if pass {
                        "resolves to"
                    } else {
                        "does not resolve to"
                    }
                ),
            ))
        }
        _ => {
            page.click(&control)?;
            let dialog = "internal:role=dialog >> nth=0";
            if page.expect_visible(dialog).is_err() {
                return Ok((false, format!("{label} did not open an accessible dialog")));
            }
            let shown = page
                .text(dialog)?
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let pass = shown.to_lowercase().contains(&target.to_lowercase());
            Ok((
                pass,
                format!(
                    "Dialog {} {}",
                    if pass { "contains" } else { "does not contain" },
                    json!(target)
                ),
            ))
        }
    }
}
