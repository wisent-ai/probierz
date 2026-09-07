use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::Value;

use crate::failure::write_private;
use crate::{specs, tui};

pub(crate) const DEFAULT_SKARBIEC_BINARY: &str =
    "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skarbiec/target/release/skarbiec";

pub(crate) struct CommandResult {
    pub status: i32,
    pub output: String,
}

pub(crate) struct Shell {
    terminal: Option<tui::Terminal>,
    marker_prefix: String,
    command_number: usize,
}

impl Shell {
    pub(crate) fn spawn(
        ready: &str,
        marker_prefix: &str,
        cwd: Option<&Path>,
        env: &BTreeMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Result<Self, String> {
        let command = format!("stty -echo; printf '{}\\n'; exec /bin/sh", ready);
        let mut spawn = tui::Spawn::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .size(cols, rows);
        if let Some(cwd) = cwd {
            spawn = spawn.cwd(cwd);
        }
        for (name, value) in env {
            spawn = spawn.env(name, value);
        }
        let terminal = tui::Terminal::spawn(spawn).map_err(|error| error.to_string())?;
        terminal
            .wait_for(ready, Duration::from_secs(15), true)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            terminal: Some(terminal),
            marker_prefix: marker_prefix.to_string(),
            command_number: 0,
        })
    }

    pub(crate) fn run_program(
        &mut self,
        binary: &str,
        args: &[&str],
        env: &[(&str, &str)],
        timeout: Duration,
    ) -> Result<CommandResult, String> {
        let command = env
            .iter()
            .map(|(name, value)| format!("{name}={}", shell_quote(value)))
            .chain(
                std::iter::once(binary)
                    .chain(args.iter().copied())
                    .map(shell_quote),
            )
            .collect::<Vec<_>>()
            .join(" ");
        self.run_command(&command, timeout)
    }

    pub(crate) fn run_command(
        &mut self,
        command: &str,
        timeout: Duration,
    ) -> Result<CommandResult, String> {
        self.command_number += 1;
        let marker = format!("{}{}_DONE__", self.marker_prefix, self.command_number);
        let terminal = self
            .terminal
            .as_mut()
            .ok_or_else(|| "the Skarbiec fixture terminal is closed".to_string())?;
        let log_start = terminal.full_log().len();
        let line = format!(
            "{command}; skarbiec_command_status=$?; printf '\\n{marker}:%s\\n' \"$skarbiec_command_status\""
        );
        terminal.send(&line).map_err(|error| error.to_string())?;
        terminal.key("enter").map_err(|error| error.to_string())?;
        terminal
            .wait_for(&marker, timeout, true)
            .map_err(|error| error.to_string())?;
        let complete = terminal.full_log();
        let command_log = complete
            .get(log_start..)
            .ok_or_else(|| "terminal log changed at a non-character boundary".to_string())?;
        let status_pattern = Regex::new(&format!(r"{}:(\d+)", regex::escape(&marker)))
            .map_err(|error| error.to_string())?;
        let status = status_pattern
            .captures(command_log)
            .and_then(|capture| capture.get(1))
            .and_then(|value| value.as_str().parse::<i32>().ok())
            .ok_or_else(|| format!("expected completion status from {command}"))?;
        let output =
            command_log[..command_log.find(&marker).unwrap_or(command_log.len())].to_string();
        Ok(CommandResult { status, output })
    }

    pub(crate) fn full_log(&self) -> String {
        self.terminal
            .as_ref()
            .map(tui::Terminal::full_log)
            .unwrap_or_default()
    }

    pub(crate) fn close(&mut self) -> Result<(), String> {
        if let Some(terminal) = self.terminal.take() {
            terminal.close().map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        if let Some(terminal) = self.terminal.take() {
            let _ = terminal.close();
        }
    }
}

pub(crate) fn binary(context: &specs::Context) -> String {
    context
        .optional("TUI_CMD")
        .unwrap_or_else(|| DEFAULT_SKARBIEC_BINARY.to_string())
}

pub(crate) fn required_binary(context: &specs::Context) -> Result<String, String> {
    context.optional("TUI_CMD").ok_or_else(|| {
        "TUI_CMD is required: provide the released Skarbiec executable; gpg and an isolated owner keyring are external runtime prerequisites".to_string()
    })
}

pub(crate) fn scratch(prefix: &str) -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = PathBuf::from("/tmp").join(format!("{prefix}-{}-{stamp}", std::process::id()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    }
    #[cfg(not(unix))]
    fs::create_dir(&path).map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    Ok(path)
}

pub(crate) fn env(pairs: &[(&str, &Path)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.to_string_lossy().into_owned()))
        .collect()
}

pub(crate) fn parse_json(output: &str, failure: impl FnOnce() -> String) -> Result<Value, String> {
    let object = output.find('{');
    let array = output.find('[');
    let start = match (object, array) {
        (Some(left), Some(right)) => left.min(right),
        (Some(index), None) | (None, Some(index)) => index,
        (None, None) => return Err(failure()),
    };
    serde_json::Deserializer::from_str(&output[start..])
        .into_iter::<Value>()
        .next()
        .ok_or_else(failure)?
        .map_err(|error| format!("invalid JSON output: {error}\n{}", &output[start..]))
}

pub(crate) fn successful_json(
    shell: &mut Shell,
    binary: &str,
    args: &[&str],
    env: &[(&str, &str)],
    timeout: Duration,
) -> Result<Value, String> {
    let description = if args.is_empty() {
        "command menu".to_string()
    } else {
        args.join(" ")
    };
    let result = shell.run_program(binary, args, env, timeout)?;
    if result.status != 0 {
        return Err(format!("skarbiec command failed: {description}"));
    }
    parse_json(&result.output, || {
        format!("expected JSON output from: {description}")
    })
}

pub(crate) fn write_trace(
    context: &specs::Context,
    file: &str,
    value: Value,
) -> Result<(), String> {
    fs::create_dir_all(&context.artifacts)
        .map_err(|error| format!("{}: {error}", context.artifacts.display()))?;
    let path = context.artifacts.join(file);
    let mut body = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    body.push(b'\n');
    write_private(&path, &body).map_err(|error| format!("{}: {error}", path.display()))?;
    context.media_typed("trace", path, "application/json");
    Ok(())
}

pub(crate) fn ensure(condition: bool, reason: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(reason.into())
    }
}

pub(crate) fn strings<'a>(value: &'a Value, pointer: &str) -> Vec<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn clean(path: &Path) {
    tui::remove_scratch(path);
}
