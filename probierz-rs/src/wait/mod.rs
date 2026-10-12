//! The one way Probierz waits for a child process.
//!
//! A child Probierz waits on is announced on stderr before the wait begins,
//! and its end or failure after, in the line Stado declares (`waits.json`, a
//! copy of Stado's declaration that Oko reads too): `czekam: <what>; gdzie:
//! <where>; rodzaj: proces; od: <UTC>; id: <pid>-<n>`, then `koniec czekania`
//! or `blad czekania`. A session's terminal and `oko sessions agents` then
//! name the command a run stands on and since when. Nothing here limits how
//! long a wait takes: it only makes the wait visible. Stdout is never
//! written, so JSON answers are untouched.

use std::fmt::Display;
use std::io::Write;
use std::sync::LazyLock;
use std::time::Instant;

use serde::Deserialize;

#[derive(Deserialize)]
struct Fields {
    #[serde(rename = "where")]
    place: String,
    kind: String,
    since: String,
    id: String,
    took: String,
    cause: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Format {
    start_key: String,
    end_key: String,
    failure_key: String,
    separator: String,
    key_separator: String,
    id_join: String,
    fields: Fields,
}

static FORMAT: LazyLock<Format> = LazyLock::new(|| {
    serde_json::from_str(include_str!("waits.json"))
        .expect("waits.json is Stado's declared wait line")
});

/// The moment this process first waited; a wait's id is the process and the
/// nanoseconds since then at which it began.
static FIRST: LazyLock<Instant> = LazyLock::new(Instant::now);

/// The word the line's kind field carries for a child process.
const PROCESS: &str = "proces";

/// A value made safe for a field: the separator's character and a line end
/// never appear inside one.
fn field(value: &str) -> String {
    let separator = FORMAT.separator.trim();
    value
        .trim()
        .chars()
        .map(|character| match character {
            '\n' | '\r' => ' ',
            other if separator.starts_with(other) => ',',
            other => other,
        })
        .collect()
}

fn pair(name: &str, value: &str) -> String {
    format!("{}{name}{}{value}", FORMAT.separator, FORMAT.key_separator)
}

fn write(line: &str) {
    // A stderr that cannot be written has nowhere to say so.
    let _ = std::io::stderr()
        .lock()
        .write_all(format!("{line}\n").as_bytes());
}

/// Wait for `work`, a child process, saying at both ends what it is and
/// where it runs.
pub(crate) fn blocking<T, E: Display>(
    what: impl Display,
    place: impl Display,
    work: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let (what, place) = (field(&what.to_string()), field(&place.to_string()));
    let names = &FORMAT.fields;
    let started = Instant::now();
    let id = format!(
        "{}{}{}",
        std::process::id(),
        FORMAT.id_join,
        started.duration_since(*FIRST).as_nanos()
    );
    let since = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    write(&format!(
        "{}{}{what}{}{}{}{}",
        FORMAT.start_key,
        FORMAT.key_separator,
        pair(&names.place, &place),
        pair(&names.kind, PROCESS),
        pair(&names.since, &since),
        pair(&names.id, &id),
    ));
    let result = work();
    let took = format!("{:.3}s", started.elapsed().as_secs_f64());
    match &result {
        Ok(_) => write(&format!(
            "{}{}{what}{}{}",
            FORMAT.end_key,
            FORMAT.key_separator,
            pair(&names.took, &took),
            pair(&names.id, &id),
        )),
        Err(error) => write(&format!(
            "{}{}{what}{}{}{}{}",
            FORMAT.failure_key,
            FORMAT.key_separator,
            pair(&names.place, &place),
            pair(&names.took, &took),
            pair(&names.cause, &field(&error.to_string())),
            pair(&names.id, &id),
        )),
    }
    result
}

/// Run `command` to its end and collect what it printed, saying so at both
/// ends: what is waited for is its command line, where is its directory.
pub(crate) fn output(command: &mut std::process::Command) -> std::io::Result<std::process::Output> {
    let what = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|part| part.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    let place = match command.get_current_dir() {
        Some(directory) => directory.display().to_string(),
        None => "the caller's directory".to_string(),
    };
    crate::wait::blocking(what, place, || command.output())
}
