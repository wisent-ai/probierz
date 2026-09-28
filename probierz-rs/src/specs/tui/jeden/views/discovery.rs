//! What jeden finds in a workspace it is pointed at. Each journey plants a
//! real file (an extension module, a custom agent) in a scratch workspace and
//! asks the app to list it, so "discovered" means read off the disk rather
//! than compiled in. The setup checklist is the same idea against a home with
//! credentials and one without.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use regex::Regex;

use super::constants::VIEW_SECONDS;
use super::replays::{launch, submit};
use crate::specs::{self, tui::common};
use crate::tui::Terminal;

/// The fixtures the jeden journeys plant.
fn fixture(context: &specs::Context, name: &str) -> PathBuf {
    context
        .harness
        .join("packages/web/harness/fixtures")
        .join(name)
}

/// The screen with box borders removed and wrapped rows joined both ways a
/// terminal splits them: glued (a path cut at the frame edge) and spaced (a
/// sentence cut across rows).
fn flattened(screen: &str) -> String {
    let rows: Vec<String> = screen
        .lines()
        .map(|row| {
            row.trim_matches(|c: char| c.is_whitespace() || "│┃║|".contains(c))
                .to_string()
        })
        .collect();
    format!("{}\n{}", rows.concat(), rows.join(" "))
}

/// A scratch workspace holding one planted file at `relative`.
fn workspace(
    context: &specs::Context,
    prefix: &str,
    relative: &str,
    name: &str,
) -> Result<PathBuf, String> {
    let cwd = common::scratch(prefix)?;
    let target = cwd.join(relative);
    fs::create_dir_all(&target).map_err(|error| format!("{}: {error}", target.display()))?;
    fs::copy(fixture(context, name), target.join(name))
        .map_err(|error| format!("fixture {name}: {error}"))?;
    Ok(cwd)
}

/// Run `steps` in a jeden session on `cwd` with a warm sandbox home, then clean both.
fn in_session(
    context: &specs::Context,
    cwd: &Path,
    credentials: bool,
    steps: impl FnOnce(&mut Terminal) -> Result<(), String>,
) -> Result<(), String> {
    let home = super::super::sandbox::home(true, credentials)?;
    let cwd_arg = cwd.display().to_string();
    let outcome = launch(context, &home, &["--cwd", &cwd_arg]).and_then(|mut app| {
        let result = steps(&mut app);
        let _ = app.close();
        result
    });
    common::remove(&home);
    outcome
}

fn until(app: &Terminal, what: &str, holds: impl Fn(&str) -> bool) -> Result<String, String> {
    app.wait_until(what, Duration::from_secs(VIEW_SECONDS), false, |screen| {
        holds(&flattened(screen))
    })
    .map_err(|error| error.detail)
}

/// jeden-extension-discovery: /extensions lists an extension module planted in the workspace.
pub fn extensions(context: &specs::Context) -> Result<(), String> {
    let cwd = workspace(
        context,
        "probierz-ext",
        ".jeden/extensions",
        "probe-ext.mjs",
    )?;
    let outcome = in_session(context, &cwd, true, |app| {
        submit(app, "/extensions")?;
        // The row carries an absolute path the frame truncates, so the check is
        // the kind of row plus the absence of the empty state.
        let screen = until(app, "a native extension row", |screen| screen.contains("Native extension"))
            .map_err(|error| format!("an extension module in .jeden/extensions is not discovered by /extensions: {error}"))?;
        if screen.contains("No extensions or plugins found") {
            return Err(format!(
                "/extensions reports no extensions beside the planted module:\n{screen}"
            ));
        }
        Ok(())
    });
    common::remove(&cwd);
    outcome
}

/// jeden-agent-discovery: /agents lists and shows a custom agent planted in the workspace.
pub fn agents(context: &specs::Context) -> Result<(), String> {
    let cwd = workspace(
        context,
        "probierz-agents",
        ".jeden/agents",
        "probe-agent.json",
    )?;
    let outcome = in_session(context, &cwd, true, |app| {
        submit(app, "/agents")?;
        until(app, "the probe-agent row", |screen| {
            screen.contains("probe-agent")
        })
        .map_err(|error| format!("/agents does not list the planted agent: {error}"))?;
        app.key("esc").map_err(|error| error.detail)?;
        submit(app, "/agents show probe-agent")?;
        until(app, "the probe agent's description", |screen| {
            screen.to_lowercase().contains("probe agent for tests")
        })
        .map_err(|error| format!("/agents show does not show the planted agent: {error}"))
        .map(drop)
    });
    common::remove(&cwd);
    outcome
}

/// jeden-setup-checklist: /setup asks for the router credentials only in a home without them.
pub fn setup_checklist(context: &specs::Context) -> Result<(), String> {
    // The TUI opens the wizard: a bare home offers "Set BRAMA_URL [INPUT]", a
    // configured one reports "BRAMA_URL configured [OK]".
    let prompts = Regex::new(r"(?i)Set BRAMA_URL.*\[INPUT\]").map_err(|error| error.to_string())?;
    let reports =
        Regex::new(r"(?i)BRAMA_URL configured.*\[OK\]").map_err(|error| error.to_string())?;
    let cwd = common::scratch("probierz-setup")?;
    let bare = in_session(context, &cwd, false, |app| {
        submit(app, "/setup")?;
        until(app, "the BRAMA_URL input prompt", |screen| {
            prompts.is_match(screen)
        })
        .map(drop)
    });
    let configured = in_session(context, &cwd, true, |app| {
        submit(app, "/setup")?;
        until(app, "BRAMA_URL reported configured", |screen| {
            reports.is_match(screen)
        })
        .map(drop)
    });
    common::remove(&cwd);
    match (bare, configured) {
        (Ok(()), Ok(())) => Ok(()),
        (bare, configured) => Err(format!(
            "/setup does not distinguish a credential-less home from a configured one: bare home {}, configured home {}",
            bare.err().unwrap_or_else(|| "prompted".into()),
            configured.err().unwrap_or_else(|| "reported OK".into())
        )),
    }
}
