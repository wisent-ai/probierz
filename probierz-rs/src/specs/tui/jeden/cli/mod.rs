//! The jeden CLI surface, hermetic: every journey runs the jeden binary
//! (TUI_CMD, or `jeden` on PATH) in a scratch workspace and never touches
//! the network. `network` holds the journeys that need a reachable Brama.

use std::collections::BTreeMap;
use std::time::Duration;

use regex::Regex;
use serde_json::Value;

use constants::{
    COMMAND_SECONDS, GALLERY_ROWS, SETTINGS_PREFILL_ROWS, SETTINGS_SECTIONS, STATS_FIELDS,
};

use crate::specs::{self, tui::common};

mod constants;
pub(crate) mod network;
pub(crate) mod perf;
mod relay;

/// Run jeden with `args` (and `input` on stdin) in a fresh scratch workspace.
fn jeden(
    context: &specs::Context,
    args: &[&str],
    input: Option<&str>,
) -> Result<common::Output, String> {
    let binary = context
        .optional("TUI_CMD")
        .unwrap_or_else(|| "jeden".to_string());
    let cwd = common::scratch("jeden-probierz")?;
    let output = common::run(
        &binary,
        &args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
        Some(&cwd),
        &BTreeMap::new(),
        &[],
        input,
        Duration::from_secs(COMMAND_SECONDS),
    );
    common::remove(&cwd);
    output
}

/// The command's stdout, refusing a failed exit.
fn succeeded(
    context: &specs::Context,
    args: &[&str],
    input: Option<&str>,
) -> Result<String, String> {
    let output = jeden(context, args, input)?;
    if output.code() != Some(0) {
        return Err(format!(
            "jeden {} exited {:?}:\n{}",
            args.join(" "),
            output.code(),
            output.combined()
        ));
    }
    Ok(output.stdout)
}

fn every(text: &str, rows: &[&str], what: &str) -> Result<(), String> {
    for row in rows {
        common::contains(text, row, format!("{what} does not show {row:?}:\n{text}"))?;
    }
    Ok(())
}

/// jeden-cli-basics: version, config, tools, completions, worktree and stats answer as documented.
pub fn basics(context: &specs::Context) -> Result<(), String> {
    if succeeded(context, &["--version"], None)?.trim().is_empty() {
        return Err("jeden --version printed nothing".into());
    }
    common::contains(
        &succeeded(context, &["config"], None)?,
        "tools.approvalMode",
        "jeden config does not print tools.approvalMode",
    )?;
    let tools = succeeded(context, &["tools"], None)?;
    common::contains(&tools, "read_file", "jeden tools does not list read_file")?;
    common::contains(&tools, "glob_paths", "jeden tools does not list glob_paths")?;
    let completions = succeeded(context, &["completions", "bash"], None)?;
    if !completions.starts_with("# bash completion for jeden")
        || !(completions.contains("complete") || completions.contains("compgen"))
    {
        return Err(format!(
            "jeden completions bash is not a bash completion script:\n{completions}"
        ));
    }
    if succeeded(context, &["worktree", "list"], None)?
        .trim()
        .is_empty()
    {
        return Err("jeden worktree list printed nothing".into());
    }
    let summary = Regex::new(r"\d+ events · \d+ tokens · cost \d+ · sessions \d+")
        .map_err(|error| error.to_string())?;
    let line = succeeded(context, &["stats", "--summary"], None)?;
    if !summary.is_match(&line) {
        return Err(format!(
            "jeden stats --summary is not the one-line snapshot:\n{line}"
        ));
    }
    let stats: Value = serde_json::from_str(&succeeded(context, &["stats", "--json"], None)?)
        .map_err(|error| format!("jeden stats --json is not JSON: {error}"))?;
    for field in STATS_FIELDS {
        if stats.pointer(field).is_none() {
            return Err(format!("jeden stats --json carries no {field}"));
        }
    }
    Ok(())
}

/// jeden-cli-settings-export: the piped /settings export groups rows and offers scalar prefill rows.
pub fn settings_export(context: &specs::Context) -> Result<(), String> {
    let out = succeeded(context, &[], Some("/settings\n"))?;
    every(&out, &SETTINGS_SECTIONS, "the /settings export")?;
    every(&out, &SETTINGS_PREFILL_ROWS, "the /settings export")
}

/// jeden-cli-gallery: gallery renders one theme's fixtures, sweeps every theme, and refuses an unknown one.
pub fn gallery(context: &specs::Context) -> Result<(), String> {
    let nord = succeeded(context, &["gallery", "--theme", "nord"], None)?;
    every(&nord, &["── theme: nord ──"], "jeden gallery --theme nord")?;
    every(&nord, &GALLERY_ROWS, "jeden gallery --theme nord")?;
    let all = succeeded(context, &["gallery", "--all"], None)?;
    let header = Regex::new(r"── theme: (\S+) ──").map_err(|error| error.to_string())?;
    let themes: Vec<&str> = header
        .captures_iter(&all)
        .filter_map(|found| found.get(1))
        .map(|name| name.as_str())
        .collect();
    if themes.len() < 2 {
        return Err(format!(
            "jeden gallery --all rendered {} theme(s):\n{all}",
            themes.len()
        ));
    }
    for theme in themes {
        succeeded(context, &["gallery", "--theme", theme], None).map_err(|error| {
            format!("gallery --all swept {theme} but gallery --theme {theme} fails: {error}")
        })?;
    }
    let bogus = jeden(context, &["gallery", "--theme", "bogus"], None)?;
    if bogus.code() == Some(0) {
        return Err("jeden gallery accepted an unknown theme".into());
    }
    Ok(())
}

/// jeden-cli-collab-share: /collab start on an http relay prints share URLs with QR codes.
pub fn collab_share(context: &specs::Context) -> Result<(), String> {
    let relay = relay::Relay::start()?;
    let script = format!(
        "/collab start http://127.0.0.1:{}\n/collab stop\n",
        relay.port()
    );
    let out = succeeded(context, &[], Some(&script))?;
    every(
        &out,
        &["View URL:", "Full write URL:", "█"],
        "/collab start on an http relay",
    )
}
