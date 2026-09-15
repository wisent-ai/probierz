//! Reading CuaDriver's own readiness without prompting, and reading
//! the product's report off the screen.
//!
//! This journey must not change the driver's permission state: it reads
//! readiness before the app launches and again after the product
//! operation, and a difference is a failure. Everything here is
//! read-only.

use super::*;

/// Where the bundled CuaDriver lives on a prepared macOS host.
const BUNDLED_DRIVER: &str = "/Applications/CuaDriver.app/Contents/MacOS/cua-driver";

/// The socket Probierz's own driver daemon listens on, relative to HOME.
const DRIVER_SOCKET: &str = "Library/Caches/cua-driver/probierz.sock";

/// Read the existing daemon's permissions with prompting switched off,
/// so the journey observes state instead of creating it.
pub(crate) fn prompt_free_readiness(context: &specs::Context) -> Result<Value, String> {
    let binary = context
        .optional("CUA_DRIVER_BIN")
        .or_else(|| std::env::var("CUA_DRIVER_BIN").ok())
        .unwrap_or_else(|| {
            let bundled = Path::new(BUNDLED_DRIVER);
            if cfg!(target_os = "macos") && bundled.is_file() {
                bundled.to_string_lossy().into_owned()
            } else {
                "cua-driver".to_string()
            }
        });
    let socket = context
        .optional("CUA_DRIVER_SOCKET")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CUA_DRIVER_SOCKET").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(DRIVER_SOCKET))
        })
        .ok_or_else(|| "HOME is required to locate the Probierz CuaDriver socket".to_string())?;
    let output = Command::new(binary)
        .arg("call")
        .arg("check_permissions")
        .arg(r#"{"prompt":false}"#)
        .arg("--socket")
        .arg(socket)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let response: Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    Ok(response.get("permissions").cloned().unwrap_or(response))
}

/// Shell-quote an argument the way the product shows it, so the command
/// on screen can be compared character for character.
pub(crate) fn quoted(argument: &str) -> String {
    if Regex::new(r"^[A-Za-z0-9_\-./:=@+,]+$")
        .unwrap()
        .is_match(argument)
    {
        argument.to_string()
    } else {
        format!(
            "\"{}\"",
            argument.replace('\\', "\\\\").replace('"', "\\\"")
        )
    }
}

/// The exact command the product must show for this host.
pub(crate) fn preparation_command(host: &str) -> String {
    format!(
        "stado host gui-automation grant-accessibility {} --apple-only --json",
        quoted(host)
    )
}

/// Lines around the refusal marker, so a failure quotes what the
/// operator would have read on screen rather than the whole tree.
pub(crate) fn exact_refusal(view: &console::View) -> String {
    /// Lines kept before and after the marker, and how much of the tail
    /// to quote when there is no marker at all.
    const BEFORE: usize = 16;
    const AFTER: usize = 8;
    const TAIL: usize = 30;

    let lines = view.tree.lines().collect::<Vec<_>>();
    let marker = lines.iter().position(|line| {
        line.contains("Apple code capture is unavailable")
            || Regex::new(r"AX\w*Button \(Dismiss\)")
                .unwrap()
                .is_match(line)
    });
    let (start, end) = marker.map_or_else(
        || (lines.len().saturating_sub(TAIL), lines.len()),
        |position| {
            (
                position.saturating_sub(BEFORE),
                lines.len().min(position.saturating_add(AFTER)),
            )
        },
    );
    lines[start..end]
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Keep every failure this journey found, not just the first, because
/// the teardown checks run after the journey has already failed.
pub(crate) fn add_failure(current: Option<String>, next: String, context: &str) -> String {
    match current {
        Some(current) => format!("{current}; additionally {context}: {next}"),
        None => next,
    }
}

/// Whether the product's report carries `name: value`.
pub(crate) fn report_item(tree: &str, name: &str, value: &str) -> bool {
    Regex::new(&format!(
        r#"(?i){}:\s*{}(?:[\s"),]|$)"#,
        regex::escape(name),
        regex::escape(value)
    ))
    .unwrap()
    .is_match(tree)
}
