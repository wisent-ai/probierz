//! First use that ends on a real computed or observed result: a
//! representation-engineering run in wisent-gradio, and managed fleet state
//! read from skarbiec-hub's own API.

use serde_json::json;

use super::{fill_control, json_object, product_base_url, walk};
use crate::specs::web::weles::{role, role_exact, text, text_exact, Page};
use crate::specs::*;

/// wisent-gradio: onboarding-first-use renders a real representation-engineering result.
pub(crate) fn wisent_gradio(context: &Context) -> Result<(), String> {
    let base = product_base_url(context, "WISENT_GRADIO_BASE_URL")?;
    let inputs = json_object(context, "WISENT_GRADIO_STEERING_VIZ_INPUTS_JSON")?;
    if inputs.is_empty() {
        return Err("WISENT_GRADIO_STEERING_VIZ_INPUTS_JSON must provide the real model and steering-viz parser-required artifacts by accessible label".into());
    }
    walk(context, base, None, |_, page| {
        page.goto("/")?;
        page.clear_storage(&["wisent.onboarding.wisent-gradio.subject"])?;
        page.reload()?;

        let inspect = role("heading", "Create and inspect a representation result");
        let open = role("button", "Open Steering visualization");
        page.expect_visible(&text_exact("First-use representation journey"))?;
        page.expect_visible(&role("heading", "See what representations reveal"))?;
        page.click(&open)?;
        page.expect_visible(&inspect)?;

        page.reload()?;
        page.expect_visible(&inspect)?;
        page.expect_absent(&text("Journey complete"))?;
        page.click(&open)?;

        for (label, value) in &inputs {
            fill_control(page, label, value)?;
        }
        page.click(&role_exact("button", "Run steering-viz"))?;

        page.expect_visible_after_run(&role("heading", "First representation result observed"))?;
        if rendered_output(page)?.is_empty() {
            page.expect_visible(&text_exact("Visualizations"))?;
        }
        page.expect_visible(&text("Journey complete"))
    })
}

/// The non-empty values of the visible controls labelled exactly "Output".
fn rendered_output(page: &Page) -> Result<Vec<String>, String> {
    let expression = r#"(() => {
        const named = (element) => {
            const names = [];
            const aria = element.getAttribute('aria-label');
            if (aria !== null) names.push(aria);
            const ids = element.getAttribute('aria-labelledby');
            if (ids !== null) {
                names.push(ids.split(/\s+/).map((id) => document.getElementById(id)).filter(Boolean)
                    .map((node) => String(node.textContent).trim()).join(' '));
            }
            if (element.id) {
                for (const label of document.querySelectorAll(`label[for="${CSS.escape(element.id)}"]`)) names.push(String(label.textContent));
            }
            const wrapping = element.closest('label');
            if (wrapping) names.push(String(wrapping.textContent));
            return names.some((name) => name.trim() === 'Output');
        };
        return [...document.querySelectorAll('input, textarea, [aria-label], [aria-labelledby], [id]')]
            .filter((control) => control instanceof HTMLElement && control.offsetParent !== null && named(control))
            .map((control) => control instanceof HTMLInputElement || control instanceof HTMLTextAreaElement
                ? control.value.trim() : String(control.textContent).trim())
            .filter((value) => value.length > 0);
    })()"#;
    let values = page.evaluate(expression)?;
    let items = values
        .as_array()
        .ok_or_else(|| format!("the Output controls read as {values}, not a list"))?;
    Ok(items
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect())
}

/// skarbiec-hub: onboarding-first-use observes real managed Skarbiec fleet state.
pub(crate) fn skarbiec_hub(context: &Context) -> Result<(), String> {
    let base = product_base_url(context, "SKARBIEC_HUB_BASE_URL")?;
    let tenant = context.required(
        "SKARBIEC_HUB_TENANT",
        "the Skarbiec tenant this journey onboards",
    )?;
    let token = context.required(
        "SKARBIEC_HUB_MANAGEMENT_TOKEN",
        "a management token for that tenant",
    )?;
    let headers = json!({ "Authorization": format!("Bearer {token}"), "x-tenant": tenant });
    let query: String = url::form_urlencoded::byte_serialize(tenant.as_bytes()).collect();
    let onboarding = format!("/v1/onboarding?tenant={query}");
    let actions = format!("/v1/onboarding/actions?tenant={query}");
    let fleet = format!("/v1/fleet/state?tenant={query}");
    walk(context, base, None, |weles, page| {
        weles.headers(headers.clone())?;
        let act = |action: &str| -> Result<(), String> {
            let body = json!({ "action": action }).to_string();
            let answer = page.request(
                "POST",
                &actions,
                json!({ "content-type": "application/json" }),
                Some(&body),
            )?;
            if answer["ok"].as_bool() == Some(true) {
                Ok(())
            } else {
                Err(format!(
                    "POST {actions} {action} answered {}: {}",
                    answer["status"], answer["body"]
                ))
            }
        };

        act("reset")?;
        page.goto(&onboarding)?;
        let journey = body(page)?["journey"].clone();
        same(&journey["status"], "in_progress", "journey.status")?;
        same(
            &journey["screen"]["id"],
            "managed-fleet",
            "journey.screen.id",
        )?;

        act("continue")?;
        page.reload()?;
        let journey = body(page)?["journey"].clone();
        same(&journey["resumed"], true, "journey.resumed")?;
        same(
            &journey["screen"]["id"],
            "control-boundaries",
            "journey.screen.id",
        )?;
        same(&journey["status"], "in_progress", "journey.status")?;

        act("continue")?;
        page.goto(&onboarding)?;
        let journey = body(page)?["journey"].clone();
        same(&journey["screen"]["id"], "fleet-state", "journey.screen.id")?;
        same(&journey["status"], "in_progress", "journey.status")?;
        same(
            &journey["actions"][0]["id"],
            "view_managed_fleet",
            "journey.actions[0].id",
        )?;

        let opened = page.goto(&fleet)?;
        match opened["status"].as_u64() {
            Some(status) if (200..300).contains(&status) => {}
            other => return Err(format!("GET {fleet} answered {other:?}")),
        }
        let document = body(page)?;
        let state = &document["fleet_state"];
        same(
            &state["managed_by"],
            "skarbiec-hub",
            "fleet_state.managed_by",
        )?;
        same(&state["tenant"], tenant.as_str(), "fleet_state.tenant")?;
        if !state["observed_at"].is_string() {
            return Err(format!(
                "fleet_state.observed_at is not a string: {}",
                state["observed_at"]
            ));
        }
        if !state["registered_workloads"].is_u64() && !state["registered_workloads"].is_i64() {
            return Err(format!(
                "fleet_state.registered_workloads is not an integer: {}",
                state["registered_workloads"]
            ));
        }
        same(
            &document["onboarding"]["completed"],
            true,
            "onboarding.completed",
        )?;
        same(
            &document["onboarding"]["completion_fact"],
            "managed_fleet_state_observed",
            "onboarding.completion_fact",
        )
    })
}

/// The page body parsed as the JSON object the API answered.
fn body(page: &Page) -> Result<Value, String> {
    let text = page.text("body")?;
    match serde_json::from_str::<Value>(&text) {
        Ok(object @ Value::Object(_)) => Ok(object),
        _ => Err(format!("response must be a JSON object: {text}")),
    }
}

fn same(seen: &Value, want: impl Into<Value>, what: &str) -> Result<(), String> {
    let want = want.into();
    if *seen == want {
        Ok(())
    } else {
        Err(format!("{what} is {seen}, expected {want}"))
    }
}
