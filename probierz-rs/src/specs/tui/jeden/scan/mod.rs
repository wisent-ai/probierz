//! The command surface scan: every slash command jeden's own /help
//! advertises is opened in tmux and measured (`probe`), and a routed model
//! reads what it painted (`judge`). Asking the app for its command list means
//! a command added tomorrow is scanned tomorrow. A second pass runs the
//! read-only subcommands from the dispatcher plus those the screens documented.
//! The run leaves a Markdown table, and fails listing every broken check.

use std::path::Path;
use std::time::Duration;

use regex::Regex;

use constants::{CHUNK, READY_SECONDS, SEED_SUBCOMMANDS, SKIP};
use judge::Reading;
use probe::{Outcome, Probe};

use super::screens::tmux::Tmux;
use crate::specs::{self, tui::common};

mod constants;
mod judge;
mod probe;

fn ready(context: &specs::Context, home: &Path, cwd: &Path) -> Result<Tmux, String> {
    let binary = context
        .optional("TUI_CMD")
        .unwrap_or_else(|| "jeden".to_string());
    let tmux = Tmux::start(&format!("{binary} --cwd {}", cwd.display()), Some(home))?;
    tmux.until(
        "the welcome screen",
        Duration::from_secs(READY_SECONDS),
        |pane| pane.contains("Tips") || pane.contains("Welcome back"),
    )?;
    Ok(tmux)
}

/// Probe `commands` a chunk per fresh session.
fn probe_all(
    context: &specs::Context,
    home: &Path,
    cwd: &Path,
    commands: &[String],
) -> Result<Vec<Probe>, String> {
    let mut probes = Vec::new();
    for chunk in commands.chunks(CHUNK) {
        let tmux = ready(context, home, cwd)?;
        for command in chunk {
            probes.push(probe::probe(&tmux, command)?);
        }
    }
    Ok(probes)
}

fn mark(outcome: Option<Outcome>) -> &'static str {
    match outcome {
        None => "—",
        Some(Outcome::Yes) => "yes",
        Some(Outcome::No) => "NO",
        Some(Outcome::OneRow) => "n/a",
    }
}

fn table(rows: &[(Probe, Option<Reading>)]) -> Vec<String> {
    let mut lines = vec![
        "| command | status | paint ms | frames | fits | ↑↓ | search | esc | note |".to_string(),
        "|---|---|---|---|---|---|---|---|---|".to_string(),
    ];
    for (probe, reading) in rows {
        let status = match reading {
            None => "silent",
            Some(reading) if reading.errored || reading.panicked => "error",
            Some(_) if probe.picker => "picker",
            Some(_) => "text",
        };
        let note = reading
            .as_ref()
            .map_or("screen never changed", |reading| reading.note.as_str());
        lines.push(format!(
            "| {} | {status} | {} | {} | {} | {} | {} | {} | {note} |",
            probe.command,
            probe.paint_ms,
            probe.frames,
            if probe.fits { "yes" } else { "NO" },
            mark(probe.navigates),
            mark(probe.filters),
            mark(probe.closes)
        ));
    }
    lines
}

/// jeden-command-scan: every advertised command paints something, fits the pane, nothing panics, and every picker moves, filters and closes.
pub fn command_scan(context: &specs::Context) -> Result<(), String> {
    super::cli::network::brama(context)?;
    let help = super::cli::succeeded(context, &[], Some("/help\n"))?;
    let listed = Regex::new(r"(?m)^(/[a-z-]+)\s\s+\S").map_err(|error| error.to_string())?;
    let advertised: Vec<String> = listed
        .captures_iter(&help)
        .map(|found| found[1].to_string())
        .collect();
    if advertised.is_empty() {
        return Err(format!(
            "/help advertised no commands, so the scan would be vacuous:\n{help}"
        ));
    }
    let bare: Vec<String> = advertised
        .iter()
        .filter(|command| !SKIP.iter().any(|(skipped, _)| skipped == command))
        .cloned()
        .collect();
    let home = super::sandbox::home(true, true)?;
    let cwd = common::scratch("probierz-scan")?;
    let scanned = (|| {
        let probes = probe_all(context, &home, &cwd, &bare)?;
        let readings = judge::read(context, &probes, &advertised)?;
        let mut subcommands: Vec<String> =
            SEED_SUBCOMMANDS.iter().map(|sub| sub.to_string()).collect();
        for reading in readings.iter().flatten() {
            for sub in &reading.subcommands {
                if !subcommands.contains(sub)
                    && advertised
                        .iter()
                        .any(|command| sub.starts_with(&format!("{command} ")))
                {
                    subcommands.push(sub.clone());
                }
            }
        }
        let sub_probes = probe_all(context, &home, &cwd, &subcommands)?;
        let sub_readings = judge::read(context, &sub_probes, &advertised)?;
        Ok::<_, String>((
            probes.into_iter().zip(readings).collect::<Vec<_>>(),
            sub_probes.into_iter().zip(sub_readings).collect::<Vec<_>>(),
        ))
    })();
    common::remove(&home);
    common::remove(&cwd);
    let (rows, sub_rows) = scanned?;

    let mut report = vec![
        "# jeden command-surface scan".to_string(),
        String::new(),
        format!(
            "scanned {} of {} advertised commands, plus {} read-only subcommands",
            rows.len(),
            advertised.len(),
            sub_rows.len()
        ),
        String::new(),
        "## bare commands".to_string(),
    ];
    report.extend(table(&rows));
    report.extend(
        SKIP.iter().map(|(command, why)| {
            format!("| {command} | skipped | — | — | — | — | — | — | {why} |")
        }),
    );
    report.extend([String::new(), "## read-only subcommands".to_string()]);
    report.extend(table(&sub_rows));
    let path = context.artifacts.join("command-scan.md");
    std::fs::write(&path, report.join("\n"))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    context.media_typed("trace", path, "text/markdown");

    let all: Vec<&(Probe, Option<Reading>)> = rows.iter().chain(&sub_rows).collect();
    let names = |keep: &dyn Fn(&Probe, Option<&Reading>) -> bool| -> Vec<String> {
        all.iter()
            .filter(|(probe, reading)| keep(probe, reading.as_ref()))
            .map(|(probe, _)| probe.command.clone())
            .collect()
    };
    let checks = [
        (
            "these commands put nothing on the screen",
            names(&|_, reading| reading.is_none()),
        ),
        (
            "these commands panicked",
            names(&|_, reading| reading.is_some_and(|reading| reading.panicked)),
        ),
        (
            "/help advertises these but the dispatcher does not know them",
            names(&|_, reading| reading.is_some_and(|reading| reading.unrouted)),
        ),
        (
            "these views opened off-screen",
            names(&|probe, reading| reading.is_some() && !probe.fits),
        ),
        (
            "↓ moved no cursor in these pickers",
            names(&|probe, _| probe.navigates == Some(Outcome::No)),
        ),
        (
            "an unmatchable search filtered nothing in these pickers",
            names(&|probe, _| probe.filters == Some(Outcome::No)),
        ),
        (
            "Esc did not close these pickers",
            names(&|probe, _| probe.closes == Some(Outcome::No)),
        ),
    ];
    let failed: Vec<String> = checks
        .iter()
        .filter(|(_, commands)| !commands.is_empty())
        .map(|(what, commands)| format!("{what}: {}", commands.join(", ")))
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("\n"))
    }
}

/// jeden-exit: /exit ends the session.
pub fn exit(context: &specs::Context) -> Result<(), String> {
    super::cli::network::brama(context)?;
    let home = super::sandbox::home(true, true)?;
    let cwd = common::scratch("probierz-exit")?;
    let ended = ready(context, &home, &cwd).and_then(|tmux| {
        tmux.submit("/exit")?;
        Ok(tmux.ended_within(Duration::from_secs(READY_SECONDS)))
    });
    common::remove(&home);
    common::remove(&cwd);
    if ended? {
        Ok(())
    } else {
        Err("/exit left the session running".into())
    }
}
