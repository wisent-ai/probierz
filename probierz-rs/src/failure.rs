//! The failure contract, in the shape a command-line tool needs it.
//!
//! An operator gets three things and nothing else: one structured line on
//! stderr that is greppable and complete, one sentence in plain words saying
//! whether this is our outage or their request, and one exit code that tells a
//! shell script the same thing without parsing anything. There is no HTTP
//! status here and deliberately no network call to a collector — a test
//! harness that hangs because its own telemetry endpoint is unreachable is the
//! failure mode this module exists to avoid.

use std::fmt;

/// The width a terminal line gets. The rule for cutting a detail is the
/// fleet's; the width is this product's, and it has always been 300.
const MAX_DETAIL_CHARS: usize = 300;

/// How a child process ended, in words: `exited N`, or `ended by signal N`
/// when it was killed and has no exit code. Never `exit null`.
pub(crate) fn ended(status: &std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exited {code}"),
        (None, Some(signal)) => format!("ended by signal {signal}"),
        (None, None) => "ended with neither an exit code nor a signal".to_string(),
    }
}

/// The vocabulary a failure may carry. Every code answers one question: whose
/// problem is this, and is retrying worth anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// The request or its inputs are wrong; retrying changes nothing.
    Invalid,
    /// A declaration this command needs does not exist or does not parse.
    Config,
    /// Something we depend on is unavailable; retrying may work.
    Unavailable,
    /// A prerequisite on this host is missing, and its owner is named.
    Prerequisite,
    /// The journey ran and refused; the evidence says why.
    Refused,
    /// Anything we could not attribute.
    Unknown,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Invalid => "invalid",
            Code::Config => "config",
            Code::Unavailable => "unavailable",
            Code::Prerequisite => "prerequisite",
            Code::Refused => "refused",
            Code::Unknown => "unknown",
        }
    }

    /// Whether a second attempt can change the answer.
    pub fn retryable(self) -> bool {
        matches!(self, Code::Unavailable)
    }

    /// The exit code a shell reads. `75` (EX_TEMPFAIL) means "ours, try
    /// again"; `1` means "yours, do not".
    pub fn exit_code(self) -> i32 {
        if self.retryable() {
            75
        } else {
            1
        }
    }

    /// The sentence an operator reads under the structured line.
    fn sentence(self) -> &'static str {
        match self {
            Code::Invalid => "Your request was refused; nothing ran.",
            Code::Config => {
                "A declaration this command needs is missing or malformed; nothing ran."
            }
            Code::Unavailable => "Something we depend on is unavailable; retrying later may work.",
            Code::Prerequisite => "This host is missing a prerequisite; the fix is named above.",
            Code::Refused => "The run completed and refused; its evidence says why.",
            Code::Unknown => "The command failed and we could not attribute the failure.",
        }
    }
}

/// One failure, with the point it happened at.
#[derive(Debug, Clone)]
pub struct Failure {
    pub point: String,
    pub code: Code,
    pub detail: String,
}

impl Failure {
    pub fn new(point: impl Into<String>, code: Code, detail: impl Into<String>) -> Self {
        Self {
            point: point.into(),
            code,
            detail: trim_detail(&detail.into()),
        }
    }

    pub fn invalid(point: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(point, Code::Invalid, detail)
    }

    pub fn config(point: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(point, Code::Config, detail)
    }

    pub fn unavailable(point: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(point, Code::Unavailable, detail)
    }

    /// The two lines an operator sees, and the code a script reads.
    pub fn report(&self) -> i32 {
        let line = serde_json::json!({
            "failure_point": self.point,
            "error_code": self.code.as_str(),
            "service": "probierz",
            "retryable": self.code.retryable(),
            "detail": self.detail,
        });
        eprintln!("probierz-failure {line}");
        eprintln!("{}", self.code.sentence());
        self.code.exit_code()
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{}: {}", self.point, self.detail)
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        let code = match error.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => Code::Config,
            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut => {
                Code::Unavailable
            }
            _ => Code::Unknown,
        };
        Self::new("io", code, error.to_string())
    }
}

impl From<serde_json::Error> for Failure {
    fn from(error: serde_json::Error) -> Self {
        Self::new("json", Code::Config, error.to_string())
    }
}

impl From<serde_yaml::Error> for Failure {
    fn from(error: serde_yaml::Error) -> Self {
        Self::new("yaml", Code::Config, error.to_string())
    }
}

/// A detail is cut at a character boundary, and says that it was cut.
fn trim_detail(detail: &str) -> String {
    let trimmed = detail.trim();
    if trimmed.chars().count() <= MAX_DETAIL_CHARS {
        return trimmed.to_string();
    }
    let kept: String = trimmed.chars().take(MAX_DETAIL_CHARS).collect();
    format!("{kept}…")
}

pub type Answer = Result<(), Failure>;

/// The failure most commands raise: a point that says where, and a detail
/// that says what. The code is `invalid` because the overwhelming majority
/// of refusals are about the request, and the other constructors are named.
pub fn fail(point: &str, detail: impl Into<String>) -> Failure {
    Failure::invalid(point, detail)
}

/// The one timestamp every command prints. Slices that each formatted their
/// own drifted apart in the last digit; one function cannot.
pub fn now_iso() -> String {
    iso_timestamp(std::time::SystemTime::now())
}

/// Any instant, in the same shape as `now_iso`.
pub fn iso_timestamp(value: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(value)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Whether this invocation asked for `--text`; `main` records it once,
/// before any command prints. A `OnceLock`: the answer is runtime input.
static TEXT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Record whether answers print as text for a person.
pub fn answer_as_text(text: bool) {
    let _ = TEXT.set(text);
}

/// One answer on stdout: pretty-printed JSON, or with `--text` the same
/// document as indented `key: value` lines, list entries as `- ` lines.
pub fn print_json<T: serde::Serialize + ?Sized>(value: &T) -> Answer {
    if !TEXT.get().copied().unwrap_or(false) {
        println!("{}", serde_json::to_string_pretty(value)?);
        return Ok(());
    }
    let mut out = String::new();
    render(&serde_json::to_value(value)?, 0, &mut out);
    print!("{out}");
    Ok(())
}

fn render(value: &serde_json::Value, depth: usize, out: &mut String) {
    use serde_json::Value;
    let pad = "  ".repeat(depth);
    let scalar = |value: &Value| match value {
        Value::Null => Some("none".to_string()),
        Value::String(text) => Some(text.clone()),
        Value::Bool(_) | Value::Number(_) => Some(value.to_string()),
        Value::Array(items) if items.is_empty() => Some("(none)".to_string()),
        Value::Object(fields) if fields.is_empty() => Some("(none)".to_string()),
        Value::Array(_) | Value::Object(_) => None,
    };
    match value {
        Value::Object(fields) => {
            for (key, field) in fields {
                match scalar(field) {
                    Some(text) => out.push_str(&format!("{pad}{key}: {text}\n")),
                    None => {
                        out.push_str(&format!("{pad}{key}:\n"));
                        render(field, depth + 1, out);
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                match scalar(item) {
                    Some(text) => out.push_str(&format!("{pad}- {text}\n")),
                    None => {
                        out.push_str(&format!("{pad}-\n"));
                        render(item, depth + 1, out);
                    }
                }
            }
        }
        other => out.push_str(&format!("{pad}{}\n", scalar(other).unwrap_or_default())),
    }
}

/// A file only its owner can read. Evidence, reports, receipts and captured
/// output all get this: the harness runs on shared hosts.
pub fn create_private(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Write bytes to an owner-only file, replacing whatever was there.
pub fn write_private(path: &std::path::Path, body: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = create_private(path)?;
    file.write_all(body)
}

