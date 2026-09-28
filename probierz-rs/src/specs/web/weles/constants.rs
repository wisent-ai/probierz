//! The fixed numbers a web journey is held to.

/// The MCP protocol revision `weles mcp` answers (weles.wisent.com/docs/mcp-sessions).
pub(crate) const MCP_PROTOCOL: &str = "2024-11-05";

/// A page that computes a real result (a model run) may take minutes. A
/// Weles wait gives up after the engine's standard thirty seconds, so a long
/// wait is thirty of those in a row: fifteen minutes before the journey
/// fails naming what the page showed.
pub(crate) const LONG_RUN_WAITS: u32 = 30;
