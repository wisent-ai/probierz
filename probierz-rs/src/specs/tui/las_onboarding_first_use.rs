use crate::{
    specs::{self, tui::common},
    tui::{Spawn, Terminal},
};
use std::time::Duration;

fn invoke(
    binary: &str,
    args: &[&str],
    marker: &str,
    temp: &std::path::Path,
) -> Result<String, String> {
    let app = Terminal::spawn(
        Spawn::new(binary)
            .args(args.iter().copied())
            .env("HOME", temp.to_string_lossy())
            .env("XDG_STATE_HOME", temp.join("state").to_string_lossy())
            .env("USER", "probierz-las-first-use"),
    )
    .map_err(|e| e.detail)?;
    app.wait_for(marker, Duration::from_secs(30), true)
        .map_err(|e| e.detail)?;
    let log = app.full_log();
    app.close().map_err(|e| e.detail)?;
    Ok(log)
}

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = common::required(context, "TUI_CMD", "TUI_CMD is required: provide the released Las CLI executable; signed release/configuration files remain operator-owned external prerequisites")?;
    let temp = common::scratch("probierz-las-first-use")?;
    let result = (|| {
        let fresh = invoke(&binary, &["onboarding"], "Status: in_progress", &temp)?;
        common::contains(
            &fresh,
            "Understand Las federation",
            format!("expected Understand Las federation: {fresh}"),
        )?;
        common::excludes(
            &fresh,
            "Status: completed",
            format!("unexpected Status: completed: {fresh}"),
        )?;
        let resumed = invoke(
            &binary,
            &["onboarding", "advance"],
            "Run your first catalogue query",
            &temp,
        )?;
        common::contains(
            &resumed,
            "Status: in_progress",
            format!("expected Status: in_progress: {resumed}"),
        )?;
        common::contains(
            &resumed,
            "Next: las list",
            format!("expected Next: las list: {resumed}"),
        )?;
        let catalogue_output = invoke(&binary, &["list"], "\"surface\":", &temp)?;
        let catalogue = common::parse_json(&catalogue_output, "las list")?;
        let rows = catalogue
            .as_array()
            .ok_or_else(|| "expected a real Las catalogue result".to_string())?;
        if rows.is_empty() {
            return Err("expected a real Las catalogue result".to_string());
        }
        for row in rows {
            if !row.get("surface").is_some_and(|v| v.is_string())
                || !row.get("summary").is_some_and(|v| v.is_string())
                || !row.get("configured").is_some_and(|v| v.is_boolean())
                || !row.get("active").is_some_and(|v| v.is_boolean())
            {
                return Err(format!("unexpected Las catalogue row: {row}"));
            }
        }
        let completed = invoke(
            &binary,
            &["onboarding", "status"],
            "Status: completed",
            &temp,
        )?;
        common::contains(
            &completed,
            "Run your first catalogue query",
            format!("expected Run your first catalogue query: {completed}"),
        )?;
        for state in [
            "Status: not_started",
            "Status: in_progress",
            "Status: skipped",
        ] {
            common::excludes(
                &completed,
                state,
                format!("unexpected {state}: {completed}"),
            )?;
        }
        Ok(())
    })();
    common::remove(&temp);
    result
}
