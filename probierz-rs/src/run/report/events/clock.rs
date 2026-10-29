//! The instants a run's events are placed at, read from the files and
//! records that state them. An instant nobody stated is not invented: it is
//! an error naming what was looked for and why it could not be read, and an
//! event carries that error beside its null instant.

use crate::run::*;
use serde_json::json;

/// When `path` was last modified, as an RFC 3339 instant in milliseconds.
pub(crate) fn modified_iso(path: &Path) -> Result<String, String> {
    let modified = fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map_err(|error| {
            format!(
                "the modification time of {} cannot be read: {error}",
                path.display()
            )
        })?;
    system_time_iso(modified)
}

/// `time` as an RFC 3339 instant in milliseconds.
pub(crate) fn system_time_iso(time: SystemTime) -> Result<String, String> {
    let elapsed = time
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("the time {time:?} precedes 1970: {error}"))?;
    let millis = i64::try_from(elapsed.as_millis()).map_err(|error| {
        format!(
            "the time {} ms after 1970 exceeds a signed millisecond count: {error}",
            elapsed.as_millis()
        )
    })?;
    millis_iso(millis)
}

/// `millis` after 1970 as an RFC 3339 instant in milliseconds.
pub(crate) fn millis_iso(millis: i64) -> Result<String, String> {
    DateTime::from_timestamp_millis(millis)
        .map(|date| date.to_rfc3339_opts(SecondsFormat::Millis, true))
        .ok_or_else(|| format!("{millis} ms after 1970 is outside the calendar's range"))
}

/// The milliseconds after 1970 that `text`, the RFC 3339 instant `what`
/// names, states.
pub(crate) fn instant_ms(text: &str, what: &str) -> Result<i64, String> {
    DateTime::parse_from_rfc3339(text)
        .map(|date| date.timestamp_millis())
        .map_err(|error| format!("{what} {text:?} is no RFC 3339 instant: {error}"))
}

/// `text`, the RFC 3339 instant `what` names, in milliseconds and UTC.
pub(crate) fn normalized_iso(text: &str, what: &str) -> Result<String, String> {
    instant_ms(text, what).and_then(millis_iso)
}

/// `event` with `field` set to `instant`, or to null beside `<field>Error`
/// naming why the instant is unknown.
pub(crate) fn with_instant(
    mut event: Value,
    field: &str,
    instant: &Result<String, String>,
) -> Value {
    if let Value::Object(fields) = &mut event {
        match instant {
            Ok(text) => {
                fields.insert(field.to_owned(), json!(text));
            }
            Err(cause) => {
                fields.insert(field.to_owned(), Value::Null);
                fields.insert(format!("{field}Error"), json!(cause));
            }
        }
    }
    event
}

/// `value` when it is a measured quantity: finite, normal and above zero.
/// A trace writes zero, or nothing a number reads as, for a time or status
/// it did not measure.
pub(crate) fn measured(value: f64) -> Option<f64> {
    (value.is_normal() && value.is_sign_positive()).then_some(value)
}
