//! Selectors and expectations over a Weles page.
//!
//! Selectors are the strings Playwright's own locators compile to, so an
//! expectation finds the element a user's accessible-role, text and label
//! queries find: `internal:role=button[name="Next"s]`, `internal:text="…"s`,
//! `internal:label="…"s`. An expectation is a selector that matches only
//! when it holds (a text filter, an enabled state), and Weles waits for it;
//! a page-side fact is an expression Weles waits to become true.

use serde_json::json;

use super::constants::LONG_RUN_WAITS;
use super::Page;
use crate::specs::*;

/// An element with this accessible role whose name contains `name`.
pub(crate) fn role(role: &str, name: &str) -> String {
    format!("internal:role={role}[name={}i]", json!(name))
}

/// An element with this accessible role and exactly this name.
pub(crate) fn role_exact(role: &str, name: &str) -> String {
    format!("internal:role={role}[name={}s]", json!(name))
}

/// An element whose text contains `value`, ignoring case.
pub(crate) fn text(value: &str) -> String {
    format!("internal:text={}i", json!(value))
}

/// An element whose whole text is exactly `value`.
pub(crate) fn text_exact(value: &str) -> String {
    format!("internal:text={}s", json!(value))
}

/// A control labelled exactly `value`.
pub(crate) fn label(value: &str) -> String {
    format!("internal:label={}s", json!(value))
}

/// `value` as a JavaScript regular-expression literal body.
fn literal(value: &str) -> String {
    regex::escape(value).replace('/', "\\/")
}

impl Page<'_> {
    pub(crate) fn expect_visible(&self, selector: &str) -> Result<(), String> {
        self.wait(selector, "visible")
    }

    pub(crate) fn expect_hidden(&self, selector: &str) -> Result<(), String> {
        self.wait(selector, "hidden")
    }

    /// Nothing matches the selector.
    pub(crate) fn expect_absent(&self, selector: &str) -> Result<(), String> {
        self.wait(selector, "detached")
    }

    /// Visible within the long-run allowance a real computation gets.
    pub(crate) fn expect_visible_after_run(&self, selector: &str) -> Result<(), String> {
        let mut last = String::new();
        for _ in 0..LONG_RUN_WAITS {
            match self.wait(selector, "visible") {
                Ok(()) => return Ok(()),
                Err(error) => last = error,
            }
        }
        Err(last)
    }

    pub(crate) fn expect_enabled(&self, selector: &str, enabled: bool) -> Result<(), String> {
        let last = selector
            .rsplit(">>")
            .next()
            .unwrap_or(selector)
            .trim_start();
        let state = match (last.starts_with("internal:role="), enabled) {
            (true, true) => "[disabled=false]",
            (true, false) => "[disabled]",
            (false, true) => ":enabled",
            (false, false) => ":disabled",
        };
        self.wait(&format!("{selector}{state}"), "attached")
    }

    /// The element's whole text, ignoring surrounding whitespace, is `want`.
    pub(crate) fn expect_text(&self, selector: &str, want: &str) -> Result<(), String> {
        let pattern = want
            .split_whitespace()
            .map(literal)
            .collect::<Vec<_>>()
            .join("\\s+");
        self.wait(
            &format!("{selector} >> internal:has-text=/^\\s*{pattern}\\s*$/"),
            "attached",
        )
    }

    /// The element's text contains `needle` (or, with `present` false, does not).
    pub(crate) fn expect_contains(
        &self,
        selector: &str,
        needle: &str,
        present: bool,
    ) -> Result<(), String> {
        let filter = if present {
            "internal:has-text"
        } else {
            "internal:has-not-text"
        };
        self.wait(
            &format!("{selector} >> {filter}=/{}/", literal(needle)),
            "attached",
        )
    }

    /// The page address matches `pattern` (or, with `matches` false, does not).
    pub(crate) fn expect_url(&self, pattern: &str, matches: bool) -> Result<(), String> {
        let test = format!("new RegExp({}).test(location.href)", json!(pattern));
        let expression = if matches { test } else { format!("!{test}") };
        self.wait_for(&expression).map(drop).map_err(|error| {
            format!(
                "the address never {} {pattern}: {error}",
                if matches { "matched" } else { "left" }
            )
        })
    }

    /// A field of the persisted journey progress equals `want`.
    pub(crate) fn expect_progress(
        &self,
        prefix: &str,
        field: &str,
        want: &str,
    ) -> Result<(), String> {
        let expression = format!(
            "(() => {{ const key = Object.keys(localStorage).find((candidate) => candidate.startsWith({})); if (!key) return false; const progress = JSON.parse(localStorage.getItem(key) || 'null'); return Boolean(progress) && progress[{}] === {}; }})()",
            json!(format!("{prefix}.progress.")),
            json!(field),
            json!(want)
        );
        self.wait_for(&expression).map(drop).map_err(|error| {
            let seen = self
                .progress(prefix)
                .map(|progress| progress[field].to_string());
            format!("{prefix} progress {field} never became {want:?} (it reads {seen:?}): {error}")
        })
    }

    /// Wait until an expression evaluated in the page is truthy.
    pub(crate) fn wait_for(&self, expression: &str) -> Result<Value, String> {
        self.weles.call(
            "weles_page_wait_for",
            json!({ "pageId": self.id, "expression": expression }),
        )
    }

    /// Reload and return once a successful response whose address contains
    /// `part` has arrived.
    pub(crate) fn reload_awaiting(&self, part: &str) -> Result<Value, String> {
        self.weles
            .call(
                "weles_page_reload",
                json!({ "pageId": self.id, "awaitResponse": part }),
            )
            .map_err(|error| format!("no successful response from {part} after reload: {error}"))
    }
}
