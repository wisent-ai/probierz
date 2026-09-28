//! First-use journeys of web products: a real product at a base URL the
//! operator provisions, walked from a cleared start to its completion fact,
//! through one Weles browser. Each journey ends with a screenshot of where it
//! stopped, passed or failed.

use serde_json::json;

use super::weles::{label, Page, Weles};
use crate::specs::*;

pub(super) mod accounts;
mod constants;
pub(super) mod dashboards;
pub(super) mod results;

/// The product's base URL from `name`: absolute, credential-free HTTPS
/// (plain HTTP only on loopback), no query or fragment.
pub(super) fn product_base_url(context: &Context, name: &str) -> Result<url::Url, String> {
    let raw = context.required(
        name,
        "the product's base URL for this real first-use journey",
    )?;
    let url = url::Url::parse(&raw).map_err(|_| format!("{name} must be an absolute URL"))?;
    let loopback = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
    );
    let secure = url.scheme() == "https" || (loopback && url.scheme() == "http");
    if !secure
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(format!(
            "{name} must be a credential-free HTTPS URL (HTTP is allowed only for loopback)"
        ));
    }
    Ok(url)
}

/// A JSON object the operator provisions in `name`.
pub(super) fn json_object(
    context: &Context,
    name: &str,
) -> Result<serde_json::Map<String, Value>, String> {
    let raw = context.required(name, "a JSON object for this real first-use journey")?;
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(object)) => Ok(object),
        _ => Err(format!("{name} must be a JSON object")),
    }
}

/// Run `body` on a new page of a new Weles browser, then keep a screenshot
/// of the page as it was when the journey ended.
pub(super) fn walk(
    context: &Context,
    base: url::Url,
    signed_in: Option<&str>,
    body: impl FnOnce(&Weles, &Page) -> Result<(), String>,
) -> Result<(), String> {
    let weles = Weles::start(context)?;
    if let Some(variable) = signed_in {
        let path = context.required(
            variable,
            "a Playwright storage-state file of a signed-in account",
        )?;
        weles.storage(Path::new(&path))?;
    }
    let page = weles.page(Some(base))?;
    let outcome = body(&weles, &page);
    let shot = context.artifacts.join(format!("{}.png", context.title));
    if page.screenshot_png(&shot).is_ok() {
        context.media_typed("screenshot", shot, "image/png");
    }
    outcome
}

/// Fill the one visible control labelled exactly `name` with `value`, the way
/// its kind takes input: files, a checked state, a chosen option, or text.
pub(super) fn fill_control(page: &Page, name: &str, value: &Value) -> Result<(), String> {
    let selector = format!("{} >> visible=true", label(name));
    let found = page.query(&selector, &["type", "role"])?;
    let count = found["count"].as_u64().unwrap_or_default();
    if count != 1 {
        return Err(format!(
            "Expected one visible control labelled {}, found {count}",
            json!(name)
        ));
    }
    let kind = found["attributes"]["type"].as_str().unwrap_or_default();
    let role = found["attributes"]["role"].as_str().unwrap_or_default();
    let words = match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    match (kind, role) {
        ("file", _) => {
            let files = match value {
                Value::Array(items) => items
                    .iter()
                    .map(|item| item.as_str().unwrap_or_default().to_string())
                    .collect(),
                _ => vec![words],
            };
            page.upload(&selector, &files)
        }
        ("checkbox" | "radio", _) => {
            let truthy = match value {
                Value::Bool(flag) => *flag,
                Value::Number(number) => number.as_f64().is_some_and(|figure| figure != 0.0),
                Value::String(text) => !text.is_empty(),
                Value::Null => false,
                Value::Array(_) | Value::Object(_) => true,
            };
            page.check(&selector, truthy)
        }
        (_, "combobox") => {
            page.fill(&selector, &words)?;
            page.press(&selector, "ArrowDown")?;
            page.press(&selector, "Enter")
        }
        _ => page.fill(&selector, &words),
    }
}
