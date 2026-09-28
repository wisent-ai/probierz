//! A terminal application inside a detached tmux session. Screen semantics
//! (what a view replaced, what stays in scrollback, which column a divider
//! sits in) need a real terminal emulator between the application and the
//! journey; tmux is that emulator, driven through its own commands.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::specs::tui::common;

/// A tmux call gets ten seconds.
const TMUX_SECONDS: u64 = 10;
/// Both applications are compared at one geometry.
const PANE_WIDTH: &str = "200";
const PANE_HEIGHT: &str = "50";

pub(crate) struct Tmux {
    name: String,
}

fn tmux(args: &[&str]) -> Result<String, String> {
    let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    let output = common::run(
        "tmux",
        &args,
        None,
        &BTreeMap::new(),
        &[],
        None,
        Duration::from_secs(TMUX_SECONDS),
    )?;
    if output.code() != Some(0) {
        return Err(format!(
            "tmux {} exited {:?}: {}",
            args.join(" "),
            output.code(),
            output.combined()
        ));
    }
    Ok(output.stdout)
}

impl Tmux {
    /// Start `command` (with HOME set to `home` when given) in a new session.
    pub(crate) fn start(command: &str, home: Option<&Path>) -> Result<Tmux, String> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let name = format!("probierz-cmp-{}-{stamp:x}", std::process::id());
        let line = match home {
            Some(home) => format!("env HOME={} {command}", home.display()),
            None => command.to_string(),
        };
        tmux(&[
            "new-session",
            "-d",
            "-s",
            &name,
            "-x",
            PANE_WIDTH,
            "-y",
            PANE_HEIGHT,
            &line,
        ])?;
        Ok(Tmux { name })
    }

    /// Type `text` and press Enter.
    pub(crate) fn submit(&self, text: &str) -> Result<(), String> {
        tmux(&["send-keys", "-t", &self.name, "-l", text])?;
        tmux(&["send-keys", "-t", &self.name, "Enter"]).map(drop)
    }

    /// Press one tmux-named key (Escape, Up, Down, Left, Right, ...).
    pub(crate) fn key(&self, name: &str) -> Result<(), String> {
        tmux(&["send-keys", "-t", &self.name, name]).map(drop)
    }

    /// Type text without Enter.
    pub(crate) fn type_text(&self, text: &str) -> Result<(), String> {
        tmux(&["send-keys", "-t", &self.name, "-l", text]).map(drop)
    }

    /// The visible pane.
    pub(crate) fn capture(&self) -> Result<String, String> {
        tmux(&["capture-pane", "-p", "-t", &self.name])
    }

    /// The visible pane plus the whole scrollback: where appended frames live.
    pub(crate) fn history(&self) -> Result<String, String> {
        tmux(&["capture-pane", "-p", "-S", "-", "-t", &self.name])
    }

    /// Capture until `holds` is true of the pane, within `limit`; the answer
    /// is the pane and how long it took.
    pub(crate) fn until(
        &self,
        what: &str,
        limit: Duration,
        holds: impl Fn(&str) -> bool,
    ) -> Result<(String, Duration), String> {
        let started = Instant::now();
        loop {
            let pane = self.capture()?;
            if holds(&pane) {
                return Ok((pane, started.elapsed()));
            }
            if started.elapsed() >= limit {
                let tail: Vec<&str> = pane
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .collect();
                let tail = tail[tail.len().saturating_sub(12)..].join("\n");
                return Err(format!(
                    "{what} did not appear within {}s; the pane showed:\n{tail}",
                    limit.as_secs()
                ));
            }
        }
    }

    /// The pane once two captures in a row agree: a mid-paint frame is not a view.
    pub(crate) fn settled(&self, limit: Duration) -> Result<String, String> {
        let mut previous = self.capture()?;
        let started = Instant::now();
        loop {
            let pane = self.capture()?;
            if pane == previous || started.elapsed() >= limit {
                return Ok(pane);
            }
            previous = pane;
        }
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = tmux(&["kill-session", "-t", &self.name]);
    }
}
