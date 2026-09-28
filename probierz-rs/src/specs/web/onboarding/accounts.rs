//! First use of products that need a signed-in account: the journey starts
//! from a Playwright storage-state file of a real account the operator
//! exported, which Weles applies to its browser before the first page.

use super::{json_object, product_base_url, walk};
use crate::specs::web::weles::{role, role_exact, text, text_exact};
use crate::specs::*;

const APP: &str = "wisent-app.onboarding.v1";
const CONSOLE: &str = "weles-console.onboarding.2026-08-04.1";
const CONSOLE_DONE: &str =
    "First-use journey complete: this real workflow receipt was opened and inspected.";

/// wisent-app: onboarding-first-use opens a real personalized Wisent home.
pub(crate) fn wisent_app(context: &Context) -> Result<(), String> {
    let base = product_base_url(context, "WISENT_APP_BASE_URL")?;
    let profile = json_object(context, "WISENT_APP_FIRST_USE_PROFILE_JSON")?;
    let field = |key: &str| profile.get(key).and_then(Value::as_str).map(str::to_string);
    let (Some(name), Some(gender), Some(age)) =
        (field("name"), field("genderLabel"), field("ageLabel"))
    else {
        return Err("WISENT_APP_FIRST_USE_PROFILE_JSON requires string name, genderLabel, and ageLabel fields".into());
    };
    walk(
        context,
        base,
        Some("WISENT_APP_STORAGE_STATE"),
        |_, page| {
            page.goto("/onboarding")?;
            page.clear_storage(&[APP])?;
            page.reload()?;

            let next = role_exact("button", "Next");
            page.expect_enabled(&next, true)?;
            page.expect_progress(APP, "current_screen_id", "welcome")?;
            page.click(&next)?;
            page.reload()?;

            page.expect_progress(APP, "current_screen_id", "personal_info")?;
            page.expect_enabled(&next, false)?;
            page.fill("internal:role=textbox >> nth=0", &name)?;
            page.click(&text_exact(&gender))?;
            page.click(&text_exact(&age))?;
            page.expect_enabled(&next, true)?;
            page.click(&next)?;

            page.expect_progress(APP, "current_screen_id", "community_tutorial")?;
            page.click(&next)?;
            page.expect_progress(APP, "current_screen_id", "create_character_tutorial")?;
            page.expect_progress(APP, "status", "in_progress")?;
            page.expect_url(r"/onboarding(?:\?|$)", true)?;

            page.click(&next)?;
            page.expect_url(r"/home(?:\?|$)", true)?;
            page.expect_visible("internal:role=heading[level=1]")?;
            page.expect_visible("internal:testid=[data-testid=\"character-card\"s] >> nth=0")?;
            page.expect_progress(APP, "status", "completed")
        },
    )
}

/// weles-console: onboarding-first-use inspects a real Weles workflow receipt.
pub(crate) fn weles_console(context: &Context) -> Result<(), String> {
    let base = product_base_url(context, "WELES_CONSOLE_BASE_URL")?;
    walk(
        context,
        base,
        Some("WELES_CONSOLE_STORAGE_STATE"),
        |_, page| {
            page.goto("/")?;
            page.clear_storage(&["weles-console.onboarding"])?;
            page.reload()?;

            let journey = role("region", "Weles first-use journey");
            let within = |selector: String| format!("{journey} >> {selector}");
            page.expect_visible(&journey)?;
            page.expect_visible(&within(role(
                "heading",
                "A queue row is a workflow promise",
            )))?;
            page.click(&within(role("button", "Continue")))?;
            page.reload()?;

            page.expect_visible(&within(role("heading", "A host claims and runs it")))?;
            page.expect_progress(CONSOLE, "current_screen_id", "host-model")?;
            page.expect_progress(CONSOLE, "status", "in_progress")?;

            page.click(&within(role("button", "Continue")))?;
            let inspect = within(role("button", "Inspect latest receipt"));
            page.expect_enabled(&inspect, true)?;
            page.expect_absent(&text(CONSOLE_DONE))?;

            page.click(&inspect)?;
            page.expect_url(r"/$", false)?;
            page.expect_text("internal:role=status", CONSOLE_DONE)?;
            page.expect_progress(CONSOLE, "status", "completed")
        },
    )
}
