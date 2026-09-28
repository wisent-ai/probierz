//! jeden-view-content: every read-only jeden view presents its own subject.
//!
//! Weak alone, strong together with the command scan: a view that renders
//! without an error still fails here when what it shows is not the thing its
//! command promises. Each view is opened in one live jeden session over a
//! real PTY, its screen is kept as evidence, and a routed model reads every
//! screen against the subject the view is declared to present (`constants`).

use std::time::Duration;

use serde_json::json;

use constants::{READY_SCREEN, START_SECONDS, VIEW_SECONDS, VIEW_SUBJECTS};

use crate::specs::{self, tui::common};
use crate::tui::{Spawn, Terminal};

mod constants;
pub(crate) mod judge;
pub(crate) mod replays;

/// Open one view and return its settled screen.
fn open(app: &mut Terminal, command: &str) -> Result<String, String> {
    app.key("esc").map_err(|error| error.detail)?;
    app.send(command).map_err(|error| error.detail)?;
    let typed = app
        .wait_for(command, Duration::from_secs(VIEW_SECONDS), false)
        .map_err(|error| error.detail)?;
    app.key("enter").map_err(|error| error.detail)?;
    app.wait_until(
        &format!("the {command} view"),
        Duration::from_secs(VIEW_SECONDS),
        false,
        |now| now != typed,
    )
    .map_err(|error| error.detail)
}

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = context
        .optional("TUI_CMD")
        .unwrap_or_else(|| "jeden".to_string());
    let mut app = Terminal::spawn(Spawn::new(binary)).map_err(|error| error.detail)?;
    let opened = (|| {
        app.wait_for(READY_SCREEN, Duration::from_secs(START_SECONDS), false)
            .map_err(|error| error.detail)?;
        let mut views = Vec::with_capacity(VIEW_SUBJECTS.len());
        for (command, subject) in VIEW_SUBJECTS {
            views.push(judge::View {
                command: command.to_string(),
                subject: subject.to_string(),
                screen: open(&mut app, command)?,
            });
        }
        Ok::<_, String>(views)
    })();
    let _ = app.close();
    let views = opened?;
    let screens: Vec<_> = views
        .iter()
        .map(|view| json!({ "command": view.command, "screen": view.screen }))
        .collect();
    common::write_trace(context, "jeden-views.json", json!(screens))?;

    let verdicts = judge::judge(context, &views)?;
    let missing: Vec<String> = verdicts
        .iter()
        .filter(|verdict| !verdict.shows_subject)
        .map(|verdict| format!("{} ({})", verdict.command, verdict.evidence))
        .collect();
    let judged: Vec<_> = verdicts
        .iter()
        .map(|verdict| json!({ "command": verdict.command, "showsSubject": verdict.shows_subject, "evidence": verdict.evidence }))
        .collect();
    common::write_trace(context, "jeden-view-verdicts.json", json!(judged))?;
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "these views rendered without their own subject matter: {}",
            missing.join("; ")
        ))
    }
}
