//! Screen behaviour of jeden over a real PTY: network-bound views show the
//! background spinner before their content, and a model turn shows the busy
//! spinner before the answer. The catalog cache is cold on purpose: a warm
//! one opens instantly and leaves no loading state to observe.

use std::time::Duration;

use crate::specs::{self, tui::common};
use crate::tui::Terminal;

use super::cli::network::brama;
use super::views::replays::{launch, submit};

/// A view gets a minute to paint; a model turn gets a minute and a half.
const VIEW_SECONDS: u64 = 60;
const TURN_SECONDS: u64 = 90;

fn spinner(screen: &str) -> bool {
    screen.contains("working…") || screen.contains("esc to cancel")
}

/// Wait for the spinner, then for `content`; the spinner must come first.
fn spinner_then(
    app: &Terminal,
    content: &str,
    holds: impl Fn(&str) -> bool,
    seconds: u64,
) -> Result<(), String> {
    app.wait_until(
        "the loading spinner",
        Duration::from_secs(VIEW_SECONDS),
        false,
        spinner,
    )
    .map_err(|error| format!("no loading state was shown: {}", error.detail))?;
    app.wait_until(content, Duration::from_secs(seconds), false, holds)
        .map(drop)
        .map_err(|error| {
            format!(
                "{content} never appeared after the loading state: {}",
                error.detail
            )
        })
}

/// Run `steps` in a jeden session on a cold sandbox home, Brama reachable.
fn cold(
    context: &specs::Context,
    steps: impl FnOnce(&mut Terminal) -> Result<(), String>,
) -> Result<(), String> {
    brama(context)?;
    let home = super::sandbox::home(false, true)?;
    let outcome = launch(context, &home, &[]).and_then(|mut app| {
        let result = steps(&mut app);
        let _ = app.close();
        result
    });
    common::remove(&home);
    outcome
}

/// jeden-model-loading-state: /model shows a loading state before the picker opens.
pub fn model_loading(context: &specs::Context) -> Result<(), String> {
    cold(context, |app| {
        submit(app, "/model --all")?;
        spinner_then(
            app,
            "the model picker",
            |screen| screen.contains("Type to search") || screen.contains("Esc close"),
            VIEW_SECONDS,
        )
    })
}

/// jeden-usage-loading-state: /usage shows a loading state before the quota rows.
pub fn usage_loading(context: &specs::Context) -> Result<(), String> {
    cold(context, |app| {
        submit(app, "/usage")?;
        spinner_then(
            app,
            "the usage view",
            |screen| screen.contains("Provider usage") || screen.contains("quota"),
            VIEW_SECONDS,
        )
    })
}

/// jeden-turn-busy-state: a model turn shows the busy spinner before the answer.
pub fn turn_busy(context: &specs::Context) -> Result<(), String> {
    cold(context, |app| {
        // The answer must not appear in the prompt or the startup chrome.
        submit(app, "Name the capital of France, one word only.")?;
        spinner_then(
            app,
            "the model's answer",
            |screen| screen.contains("Paris"),
            TURN_SECONDS,
        )
    })
}
