//! Replays of user-reported jeden flows, kept executable after their fixes:
//! the `/model` picker that stayed silent after `/login` until the user hit
//! ^C, and the agent not knowing it is jeden.

use super::constants::{FIRST_RUN_SCREEN, READY_SCREEN, START_SECONDS};
use crate::specs::{self, tui::common};
use crate::tui::{Spawn, Terminal};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PICKER: &str = "Select model route";

/// Start jeden with `args` on the sandbox `home` and wait until it is ready.
pub(crate) fn launch(
    context: &specs::Context,
    home: &Path,
    args: &[&str],
) -> Result<Terminal, String> {
    let binary = context
        .optional("TUI_CMD")
        .unwrap_or_else(|| "jeden".to_string());
    let spawn = Spawn::new(binary)
        .args(args.iter().copied())
        .env("HOME", home.display().to_string());
    let app = Terminal::spawn(spawn).map_err(|error| error.detail)?;
    // A fresh sandbox home may open on the first-run tips instead of the
    // returning-user welcome; either means jeden is ready for input.
    app.wait_until(
        "the jeden ready screen",
        Duration::from_secs(START_SECONDS),
        false,
        |screen| screen.contains(READY_SCREEN) || screen.contains(FIRST_RUN_SCREEN),
    )
    .map_err(|error| format!("jeden did not reach its welcome screen: {}", error.detail))?;
    Ok(app)
}

pub(crate) fn submit(app: &mut Terminal, command: &str) -> Result<(), String> {
    app.send(command).map_err(|error| error.detail)?;
    app.key("enter").map_err(|error| error.detail)
}

/// How long the model picker takes to open after `command`, cold catalog.
fn picker_after(app: &mut Terminal) -> Result<Duration, String> {
    let started = Instant::now();
    submit(app, "/model")?;
    app.wait_for(PICKER, Duration::from_secs(START_SECONDS), false)
        .map_err(|error| error.detail)?;
    Ok(started.elapsed())
}

/// jeden-model-picker-after-login: /login then /model opens the picker instead of hanging.
pub fn model_picker_after_login(context: &specs::Context) -> Result<(), String> {
    let operator_env = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".jeden/.env");
    let brama = context.optional("BRAMA_URL").is_some()
        || fs::read_to_string(&operator_env).is_ok_and(|text| text.contains("BRAMA_URL="));
    if !brama {
        return Err("BRAMA_URL is required (set it, or in ~/.jeden/.env): the model picker lists Brama's catalog".into());
    }
    // A cold catalog on purpose: a warm cache would make the replay pass for
    // the wrong reason. The standalone run first tells a slow catalog apart
    // from a view queued behind the previous one, which was the report.
    let alone_home = super::super::sandbox::home(false, true)?;
    let standalone = launch(context, &alone_home, &[]).and_then(|mut app| {
        let measured = picker_after(&mut app);
        let _ = app.close();
        measured
    });
    common::remove(&alone_home);
    let standalone = standalone?;

    let home = super::super::sandbox::home(false, true)?;
    let replay = launch(context, &home, &[]).and_then(|mut app| {
        let result = (|| {
            submit(&mut app, "/login")?;
            app.wait_until(
                "the authentication status",
                Duration::from_secs(START_SECONDS),
                false,
                |screen| screen.to_lowercase().contains("authentication status"),
            )
            .map_err(|error| error.detail)?;
            app.key("esc").map_err(|error| error.detail)?;
            picker_after(&mut app)
        })();
        let _ = app.close();
        result
    });
    common::remove(&home);
    let after_login = replay.map_err(|error| {
        format!(
            "the model picker did not open after /login ({error}); standalone it opened in {}ms",
            standalone.as_millis()
        )
    })?;
    common::write_trace(
        context,
        "replay-hang.json",
        serde_json::json!({ "afterLoginMs": after_login.as_millis() as u64, "standaloneMs": standalone.as_millis() as u64 }),
    )
}

/// jeden-identity: /prompt shows that the agent is jeden.
pub fn identity(context: &specs::Context) -> Result<(), String> {
    let home = super::super::sandbox::home(false, true)?;
    let shown = launch(context, &home, &[]).and_then(|mut app| {
        let result = (|| {
            submit(&mut app, "/prompt")?;
            app.wait_until(
                "the jeden identity",
                Duration::from_secs(START_SECONDS),
                false,
                |screen| screen.to_lowercase().contains("jeden"),
            )
            .map_err(|error| {
                format!(
                    "/prompt did not surface the jeden identity: {}",
                    error.detail
                )
            })
        })();
        let _ = app.close();
        result
    });
    common::remove(&home);
    shown.map(drop)
}
