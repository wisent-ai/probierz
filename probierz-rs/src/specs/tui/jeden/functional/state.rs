//! Commands whose effect a later view must still show: todos, branches,
//! plan mode, session names and checkpoints.

use regex::Regex;

use super::{journey, marker, run};
use crate::specs::{self, tui::common};

/// Open `reader` after `writer` and require `expected` on its screen.
fn roundtrip(
    context: &specs::Context,
    writer: String,
    reader: &str,
    expected: String,
) -> Result<(), String> {
    journey(context, |app, _, _| {
        run(app, &writer)?;
        let screen = run(app, reader)?;
        common::contains(
            &screen,
            &expected,
            format!("{writer} was accepted but {reader} does not show {expected:?}:\n{screen}"),
        )
    })
}

/// jeden-todo-roundtrip: /todo add writes a todo that the reopened view still shows.
pub fn todo(context: &specs::Context) -> Result<(), String> {
    let item = marker("probierz-");
    roundtrip(context, format!("/todo add {item}"), "/todo", item)
}

/// jeden-branch-roundtrip: /branch creates a lineage node that /tree lists.
pub fn branch(context: &specs::Context) -> Result<(), String> {
    let name = marker("probe");
    roundtrip(context, format!("/branch {name}"), "/tree", name)
}

/// jeden-rename-roundtrip: /rename sticks and the session view reports the new name.
pub fn rename(context: &specs::Context) -> Result<(), String> {
    let name = marker("probe-");
    journey(context, |app, _, _| {
        let answer = run(app, &format!("/rename {name}"))?;
        if !answer.to_lowercase().contains("renamed") {
            return Err(format!(
                "/rename {name} did not report the rename:\n{answer}"
            ));
        }
        let screen = run(app, "/session")?;
        common::contains(
            &screen,
            &name,
            format!("/rename {name} reported success but /session does not show it:\n{screen}"),
        )
    })
}

/// jeden-mode-roundtrip: /plan on is still on when /plan status is asked afterwards.
pub fn plan_mode(context: &specs::Context) -> Result<(), String> {
    journey(context, |app, _, _| {
        let enabled = run(app, "/plan on")?;
        if !enabled.to_lowercase().contains("plan mode enabled") {
            return Err(format!(
                "/plan on did not report the mode as enabled:\n{enabled}"
            ));
        }
        let status = run(app, "/plan status")?;
        if !status.to_lowercase().contains("enabled") {
            return Err(format!(
                "/plan on reported success but /plan status does not report it enabled:\n{status}"
            ));
        }
        Ok(())
    })
}

/// jeden-checkpoint-roundtrip: /checkpoint mints a fresh durable checkpoint id every time.
pub fn checkpoint(context: &specs::Context) -> Result<(), String> {
    let created =
        Regex::new(r"(?i)Checkpoint (event-\S+) created").map_err(|error| error.to_string())?;
    journey(context, |app, _, _| {
        let mut ids = Vec::new();
        for _ in 0..2 {
            let screen = run(app, "/checkpoint")?;
            let id = created
                .captures(&screen)
                .and_then(|found| found.get(1))
                .map(|id| id.as_str().to_string())
                .ok_or_else(|| format!("/checkpoint did not report creating one:\n{screen}"))?;
            ids.push(id);
        }
        // Views replace each other, so what must hold is that each call mints
        // a distinct durable event id rather than reusing the previous one.
        if ids[0] == ids[1] {
            return Err(format!(
                "/checkpoint did not mint distinct ids ({} then {})",
                ids[0], ids[1]
            ));
        }
        Ok(())
    })
}
