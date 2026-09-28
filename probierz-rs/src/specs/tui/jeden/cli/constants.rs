//! What the jeden CLI journeys are held to. These constants are the declared
//! outputs of jeden's own commands (settings group headers, prefill rows,
//! gallery fixture rows) and the limits the journeys give a command.

/// A jeden command gets a minute.
pub(super) const COMMAND_SECONDS: u64 = 60;
/// Generous ceilings, so a gross regression trips without flaking on shared
/// hardware: a local command, and one that reaches Brama, in milliseconds.
pub(super) const LOCAL_CEILING_MS: u128 = 15_000;
pub(super) const NETWORK_CEILING_MS: u128 = 45_000;

/// The settings export's tab sections and group headers.
pub(super) const SETTINGS_SECTIONS: [&str; 6] = [
    "── tools (",
    "── commands (",
    "── startup (",
    "── secrets (",
    "── context (",
    "── ui (",
];
/// Scalar settings the export offers as editable prefill rows.
pub(super) const SETTINGS_PREFILL_ROWS: [&str; 2] = [
    "context.maxBytes: set value [INPUT]",
    "secrets.minLength: set value [INPUT]",
];
/// Rows the gallery's fixtures render for one theme.
pub(super) const GALLERY_ROWS: [&str; 4] = [
    "Select model route",
    "● All",
    "│",
    "Confirm destructive action",
];
/// Fields `stats --json` must carry, as JSON pointers.
pub(super) const STATS_FIELDS: [&str; 4] = ["/version", "/usage/project", "/quota", "/sessions"];
/// The model the network run asks for.
pub(super) const RUN_MODEL: &str = "codex/gpt-5.6-sol";

/// What the loopback collab relay stub answers: a POSTed event is
/// acknowledged with a sequence number, a read gets an empty event page.
pub(super) const RELAY_POSTED: &str = r#"{"seq":1}"#;
pub(super) const RELAY_EMPTY_PAGE: &str = r#"{"events":[],"cursor":0}"#;
