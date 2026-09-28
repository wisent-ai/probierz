//! Dashboards whose first use ends on live data: the journey holds the live
//! feed back (an empty or failing answer) to prove the journey waits for it,
//! then lets the real feed through and sees the result arrive.

use super::constants::{JSON, NO_OFFERS, NO_TOKENS, OK, REPORT_HELD_BACK, UNAVAILABLE};
use super::{product_base_url, walk};
use crate::specs::web::weles::{role, text};
use crate::specs::*;

const ADAM: &str = "adam-monitor.onboarding";
const MARKET: &str = "compute-marketplace.onboarding";
const TRADE: &str = "wisent-trade.first-use.v1";

/// adam-monitor: onboarding-first-use observes a real agent-economy dashboard.
pub(crate) fn adam_monitor(context: &Context) -> Result<(), String> {
    let base = product_base_url(context, "ADAM_MONITOR_BASE_URL")?;
    walk(context, base, None, |_, page| {
        page.route("**/api/report", UNAVAILABLE, JSON, REPORT_HELD_BACK)?;
        page.goto("/")?;
        page.clear_storage(&[ADAM])?;
        page.reload()?;

        page.expect_visible("#onboarding-panel")?;
        page.expect_text("#onboarding-title", "What this monitor covers")?;
        page.expect_progress(ADAM, "status", "in_progress")?;

        page.click("#onboarding-action")?;
        page.reload()?;
        page.expect_text("#onboarding-title", "Read the live evidence")?;
        page.expect_enabled("#onboarding-action", false)?;
        page.expect_progress(ADAM, "current_screen_id", "live-evidence")?;
        page.expect_progress(ADAM, "status", "in_progress")?;

        page.unroute("**/api/report")?;
        page.reload_awaiting("/api/report")?;
        page.expect_contains("#platform-overview", "Economy", true)?;
        page.expect_contains("#platform-overview", "Agents", true)?;
        page.expect_enabled("#onboarding-action", true)?;

        page.click("#onboarding-action")?;
        page.expect_hidden("#onboarding-panel")?;
        page.expect_progress(ADAM, "status", "completed")
    })
}

/// compute-marketplace: onboarding-first-use observes a real authorized machine offer.
pub(crate) fn compute_marketplace(context: &Context) -> Result<(), String> {
    let base = product_base_url(context, "COMPUTE_MARKETPLACE_BASE_URL")?;
    walk(context, base, None, |_, page| {
        page.route("**/api/v1/offers*", OK, JSON, NO_OFFERS)?;
        page.goto("/marketplace")?;
        page.clear_storage(&[MARKET])?;
        page.reload()?;

        page.expect_visible(&role(
            "heading",
            "Find authorized compute without losing control",
        ))?;
        page.click(&role("button", "How offers work"))?;
        page.reload()?;

        page.expect_visible(&role("heading", "An offer is not a running workload"))?;
        page.expect_progress(MARKET, "current_screen_id", "control_model")?;
        page.expect_progress(MARKET, "status", "in_progress")?;

        page.click(&role("button", "Inspect live offers"))?;
        page.expect_visible(&role("heading", "Inspect a real machine offer"))?;
        page.expect_visible(&text("Waiting for a live offer from the marketplace…"))?;
        page.expect_absent(&text("You have reached a live machine offer"))?;

        page.unroute("**/api/v1/offers*")?;
        page.reload_awaiting("/api/v1/offers")?;

        page.expect_visible(&text("You have reached a live machine offer"))?;
        let first_offer = "#marketplace-offers tbody tr >> nth=0";
        page.expect_visible(first_offer)?;
        page.expect_contains(first_offer, "No GPUs available", false)?;
        page.expect_visible(&format!("{first_offer} >> {}", role("button", "Rent")))?;
        page.expect_progress(MARKET, "status", "completed")
    })
}

/// wisent-trade: onboarding-first-use observes a real agent-economy market result.
pub(crate) fn wisent_trade(context: &Context) -> Result<(), String> {
    let base = product_base_url(context, "WISENT_TRADE_BASE_URL")?;
    walk(context, base, None, |_, page| {
        page.route("**/api/tokens", OK, JSON, NO_TOKENS)?;
        page.goto("/")?;
        page.clear_storage(&["wisent-trade.onboarding", TRADE])?;
        page.reload()?;

        let completion = role("region", "First-use journey complete");
        page.expect_visible(&role("heading", "How the agent economy moves"))?;
        page.expect_absent(&completion)?;
        page.click(&role("button", "Show me the live economy"))?;
        page.reload()?;

        page.expect_visible(&role("heading", "Observe a live economy result"))?;
        page.expect_visible(&text(
            "Waiting for the market to return its first listed agent token.",
        ))?;
        page.expect_progress(TRADE, "current_screen_id", "observe-result")?;
        page.expect_progress(TRADE, "status", "in_progress")?;

        page.unroute("**/api/tokens")?;
        page.reload_awaiting("/api/tokens")?;

        page.expect_visible(&completion)?;
        page.expect_contains(&completion, "Live agent economy observed", true)?;
        page.expect_contains(&completion, "AGENT", true)?;
        page.expect_contains("#live-economy", "Live Tokens", true)?;
        page.expect_visible("a[href^=\"/token/\"] >> nth=0")?;
        page.expect_progress(TRADE, "status", "completed")
    })
}
