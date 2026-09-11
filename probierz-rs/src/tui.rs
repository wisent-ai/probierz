//! The terminal driver every TUI journey runs through.
//!
//! A terminal application behaves differently without a controlling terminal:
//! it stops repainting, hides its cursor keys, and often refuses to draw at
//! all. So a journey gets a real PTY, its keystrokes arrive on that PTY, and
//! what a spec asserts against is what a human would see — the last repaint
//! frame, with the escape sequences that drew it removed.
//!
//! There is no dependency here beyond the standard library and the `script`
//! utility every supported host ships: allocating a PTY through it keeps this
//! driver honest about what the application receives, and keeps the crate free
//! of a terminal-emulation dependency it would otherwise carry for one purpose.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::failure::{Code, Failure};

mod screen;
#[cfg(test)]
mod tests;

use screen::last_frame;
pub use screen::strip_ansi;

/// The keys a journey may send by name, in the bytes a terminal sends for them.
const KEYS: [(&str, &str); 10] = [
    ("enter", "\r"),
    ("tab", "\t"),
    ("esc", "\x1b"),
    ("backspace", "\x7f"),
    ("ctrl-c", "\x03"),
    ("ctrl-d", "\x04"),
    ("up", "\x1b[A"),
    ("down", "\x1b[B"),
    ("right", "\x1b[C"),
    ("left", "\x1b[D"),
];

/// One terminal application under test.
pub struct Terminal {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    log: Arc<Mutex<String>>,
    command: String,
}

/// How the application is started.
pub struct Spawn {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
}

impl Spawn {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            cols: 120,
            rows: 36,
        }
    }

    pub fn arg(mut self, value: impl Into<String>) -> Self {
        self.args.push(value.into());
        self
    }

    pub fn args<I: IntoIterator<Item = S>, S: Into<String>>(mut self, values: I) -> Self {
        self.args.extend(values.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    pub fn cwd(mut self, path: impl Into<PathBuf>) -> Self {
        self.cwd = Some(path.into());
        self
    }

    pub fn size(mut self, cols: u16, rows: u16) -> Self {
        self.cols = cols;
        self.rows = rows;
        self
    }
}

impl Terminal {
    /// Start the application on a real PTY of the requested size.
    pub fn spawn(spec: Spawn) -> Result<Self, Failure> {
        // `script` is the portable way to hand a child a controlling terminal:
        // BSD `script -q /dev/null cmd args...` on macOS, GNU `script -qefc`
        // elsewhere. The size is set inside the session so the application
        // reads the same geometry a human would give it.
        let inner = format!(
            "stty rows {rows} cols {cols} 2>/dev/null; exec \"$@\"",
            rows = spec.rows,
            cols = spec.cols
        );
        let mut command = Command::new("script");
        if cfg!(target_os = "macos") {
            command.args(["-q", "/dev/null", "/bin/sh", "-c", &inner, "sh"]);
        } else {
            command.args([
                "-qefc",
                &format!("/bin/sh -c {}", shell_quote(&inner)),
                "/dev/null",
            ]);
        }
        command.arg(&spec.command);
        for argument in &spec.args {
            command.arg(argument);
        }
        if let Some(directory) = &spec.cwd {
            command.current_dir(directory);
        }
        command
            .env("TERM", "xterm-256color")
            .env("COLUMNS", spec.cols.to_string())
            .env("LINES", spec.rows.to_string());
        for (name, value) in &spec.env {
            command.env(name, value);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                Failure::new(
                    "tui.spawn",
                    Code::Prerequisite,
                    format!("cannot start {} through a terminal: {error}", spec.command),
                )
            })?;

        let log = Arc::new(Mutex::new(String::new()));
        for stream in [
            child.stdout.take().map(StreamKind::Out),
            child.stderr.take().map(StreamKind::Err),
        ]
        .into_iter()
        .flatten()
        {
            let sink = Arc::clone(&log);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                let mut reader: Box<dyn Read + Send> = match stream {
                    StreamKind::Out(handle) => Box::new(handle),
                    StreamKind::Err(handle) => Box::new(handle),
                };
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
                            if let Ok(mut guard) = sink.lock() {
                                guard.push_str(&text);
                            }
                        }
                    }
                }
            });
        }
        let stdin = child.stdin.take();
        let printed = std::iter::once(spec.command.clone())
            .chain(spec.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        Ok(Self {
            child,
            stdin,
            log,
            command: printed,
        })
    }

    /// Everything written so far, escape sequences removed.
    pub fn full_log(&self) -> String {
        strip_ansi(&self.raw())
    }

    /// What is on the screen now.
    pub fn screen(&self) -> String {
        last_frame(&self.raw())
    }

    fn raw(&self) -> String {
        self.log
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Type text into the application.
    pub fn send(&mut self, text: &str) -> Result<(), Failure> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(Failure::new(
                "tui.send",
                Code::Refused,
                "the application's input is closed",
            ));
        };
        stdin.write_all(text.as_bytes())?;
        stdin.flush()?;
        Ok(())
    }

    /// Press one named key.
    pub fn key(&mut self, name: &str) -> Result<(), Failure> {
        let Some((_, bytes)) = KEYS.iter().find(|(key, _)| *key == name) else {
            let known = KEYS
                .iter()
                .map(|(key, _)| *key)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(Failure::invalid(
                "tui.key",
                format!("unknown key: {name} (one of {known})"),
            ));
        };
        self.send(bytes)
    }

    /// Wait until the screen — or the whole session, when asked — contains
    /// `needle`. A timeout reports what was on the screen instead, because a
    /// journey that fails at "waiting for X" with no screen is unreadable.
    pub fn wait_for(
        &self,
        needle: &str,
        timeout: Duration,
        use_full_log: bool,
    ) -> Result<String, Failure> {
        let deadline = Instant::now() + timeout;
        loop {
            let value = if use_full_log {
                self.full_log()
            } else {
                self.screen()
            };
            if value.contains(needle) {
                return Ok(value);
            }
            if Instant::now() >= deadline {
                return Err(Failure::new(
                    "tui.wait_for",
                    Code::Refused,
                    format!(
                        "waiting for {needle:?} in `{}` timed out after {}ms\n--- screen ---\n{}",
                        self.command,
                        timeout.as_millis(),
                        self.screen()
                    ),
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// End the application and report its exit code and full session.
    pub fn close(mut self) -> Result<(Option<i32>, String), Failure> {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let status = self.child.wait()?;
        // The reader threads may still be draining; give them the moment they
        // need so a closing assertion sees the last line.
        std::thread::sleep(Duration::from_millis(100));
        Ok((status.code(), self.full_log()))
    }
}

enum StreamKind {
    Out(std::process::ChildStdout),
    Err(std::process::ChildStderr),
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Delete a journey's scratch directory, ignoring a directory that is
/// already gone: cleanup runs on the failure path too.
pub fn remove_scratch(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}
