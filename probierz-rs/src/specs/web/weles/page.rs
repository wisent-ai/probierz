//! One page of a Weles browser: the operations a journey performs on it.
//! Selectors are Playwright selector strings Weles evaluates (see `expect`
//! for the role, text and label forms).

use serde_json::json;

use super::Weles;
use crate::specs::*;

pub(crate) struct Page<'w> {
    pub(super) weles: &'w Weles,
    pub(super) id: String,
    base: Option<url::Url>,
}

impl<'w> Page<'w> {
    pub(super) fn new(weles: &'w Weles, id: String, base: Option<url::Url>) -> Self {
        Page { weles, id, base }
    }

    fn call(&self, tool: &str, mut arguments: Value) -> Result<Value, String> {
        arguments["pageId"] = Value::String(self.id.clone());
        self.weles.call(tool, arguments)
    }

    /// An absolute address, or one relative to the journey's base URL.
    pub(crate) fn address(&self, target: &str) -> Result<String, String> {
        if let Ok(absolute) = url::Url::parse(target) {
            return Ok(absolute.to_string());
        }
        let base = self
            .base
            .as_ref()
            .ok_or_else(|| format!("{target} is relative and this journey has no base URL"))?;
        base.join(target)
            .map(|joined| joined.to_string())
            .map_err(|error| format!("{target} does not resolve against {base}: {error}"))
    }

    /// Navigate; answers { url, status, title }.
    pub(crate) fn goto(&self, target: &str) -> Result<Value, String> {
        let url = self.address(target)?;
        self.call("weles_page_goto", json!({ "url": url }))
    }

    pub(crate) fn reload(&self) -> Result<Value, String> {
        self.call("weles_page_reload", json!({}))
    }

    pub(crate) fn text(&self, selector: &str) -> Result<String, String> {
        self.weles.call_text(
            "weles_page_text",
            json!({ "pageId": self.id, "selector": selector }),
        )
    }

    /// { count, visible, enabled, text, attributes, checked?, value? } of the
    /// first element the selector matches.
    pub(crate) fn query(&self, selector: &str, attributes: &[&str]) -> Result<Value, String> {
        self.call(
            "weles_page_query",
            json!({ "selector": selector, "attributes": attributes }),
        )
    }

    pub(crate) fn click(&self, selector: &str) -> Result<(), String> {
        self.call("weles_page_click", json!({ "selector": selector }))
            .map(drop)
    }

    pub(crate) fn fill(&self, selector: &str, value: &str) -> Result<(), String> {
        self.call(
            "weles_page_fill",
            json!({ "selector": selector, "value": value }),
        )
        .map(drop)
    }

    pub(crate) fn press(&self, selector: &str, key: &str) -> Result<(), String> {
        self.call(
            "weles_page_press",
            json!({ "selector": selector, "key": key }),
        )
        .map(drop)
    }

    pub(crate) fn check(&self, selector: &str, checked: bool) -> Result<(), String> {
        self.call(
            "weles_page_check",
            json!({ "selector": selector, "checked": checked }),
        )
        .map(drop)
    }

    pub(crate) fn upload(&self, selector: &str, files: &[String]) -> Result<(), String> {
        self.call(
            "weles_page_upload",
            json!({ "selector": selector, "files": files }),
        )
        .map(drop)
    }

    /// Wait until the element is attached, detached, visible or hidden.
    pub(crate) fn wait(&self, selector: &str, state: &str) -> Result<(), String> {
        self.call(
            "weles_page_wait",
            json!({ "selector": selector, "state": state }),
        )
        .map(drop)
        .map_err(|error| format!("{selector} did not become {state}: {error}"))
    }

    /// The JSON value of a JavaScript expression evaluated in the page.
    pub(crate) fn evaluate(&self, expression: &str) -> Result<Value, String> {
        self.call("weles_page_evaluate", json!({ "expression": expression }))
    }

    pub(crate) fn viewport(&self, width: u32, height: u32) -> Result<(), String> {
        self.call(
            "weles_page_viewport",
            json!({ "width": width, "height": height }),
        )
        .map(drop)
    }

    pub(crate) fn init_script(&self, script: &str) -> Result<(), String> {
        self.call("weles_page_init_script", json!({ "script": script }))
            .map(drop)
    }

    /// Answer requests matching the glob with this response until unrouted.
    pub(crate) fn route(
        &self,
        pattern: &str,
        status: u16,
        content_type: &str,
        body: &str,
    ) -> Result<(), String> {
        let arguments = json!({ "pattern": pattern, "status": status, "contentType": content_type, "body": body });
        self.call("weles_page_route", arguments).map(drop)
    }

    pub(crate) fn unroute(&self, pattern: &str) -> Result<(), String> {
        self.call("weles_page_unroute", json!({ "pattern": pattern }))
            .map(drop)
    }

    /// { consoleErrors, failedRequests, responses: [{ url, status, method }] }.
    pub(crate) fn events(&self, clear: bool) -> Result<Value, String> {
        self.call("weles_page_events", json!({ "clear": clear }))
    }

    /// A request carrying the page's cookies and headers; { url, status, ok, body }.
    pub(crate) fn request(
        &self,
        method: &str,
        target: &str,
        headers: Value,
        body: Option<&str>,
    ) -> Result<Value, String> {
        let mut arguments =
            json!({ "method": method, "url": self.address(target)?, "headers": headers });
        if let Some(body) = body {
            arguments["body"] = Value::String(body.to_string());
        }
        self.call("weles_page_request", arguments)
    }

    /// A JPEG of the viewport, written to `path`.
    pub(crate) fn screenshot_jpeg(&self, path: &Path, quality: u8) -> Result<(), String> {
        let arguments =
            json!({ "path": path, "type": "jpeg", "quality": quality, "fullPage": false });
        self.call("weles_page_screenshot", arguments).map(drop)
    }

    /// A PNG of the viewport, written to `path`.
    pub(crate) fn screenshot_png(&self, path: &Path) -> Result<(), String> {
        self.call(
            "weles_page_screenshot",
            json!({ "path": path, "type": "png", "fullPage": false }),
        )
        .map(drop)
    }

    /// Remove the localStorage keys a product owns, so a journey starts new.
    pub(crate) fn clear_storage(&self, prefixes: &[&str]) -> Result<(), String> {
        let expression = format!(
            "(() => {{ const owned = {}; for (const key of Object.keys(localStorage)) {{ if (owned.some((prefix) => key.startsWith(prefix))) localStorage.removeItem(key); }} return true; }})()",
            json!(prefixes)
        );
        self.evaluate(&expression).map(drop)
    }

    /// The journey progress a product persists under `<prefix>.progress.*`.
    pub(crate) fn progress(&self, prefix: &str) -> Result<Value, String> {
        let expression = format!(
            "(() => {{ const owned = {prefix}; const key = Object.keys(localStorage).find((candidate) => candidate.startsWith(`${{owned}}.progress.`)); if (!key) throw new Error(`No persisted journey progress for ${{owned}}`); const parsed = JSON.parse(localStorage.getItem(key) || 'null'); if (!parsed || typeof parsed !== 'object') throw new Error(`Invalid persisted journey progress for ${{owned}}`); return parsed; }})()",
            prefix = json!(prefix)
        );
        self.evaluate(&expression)
    }

    pub(crate) fn location(&self) -> Result<String, String> {
        self.evaluate("location.href")?
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| "location.href is not a string".to_string())
    }
}
