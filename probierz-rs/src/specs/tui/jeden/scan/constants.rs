//! The command surface scan's constants: its waiting budgets, the commands it
//! declines to run (with the reason the report prints) and the read-only
//! subcommands taken from jeden's own dispatcher.

/// Commands per tmux session: a fresh session bounds mode and state bleed
/// (toggles such as /plan and /fast) without paying startup for every command.
pub(super) const CHUNK: usize = 12;
/// A command gets fifteen seconds to paint and to settle, and a session a minute to start.
pub(super) const PAINT_SECONDS: u64 = 15;
pub(super) const READY_SECONDS: u64 = 60;
/// Typed into a picker's search to prove it filters: no row can match it.
pub(super) const UNMATCHABLE_QUERY: &str = "qzxwvj";
/// A picker with one row cannot demonstrate moving or filtering.
pub(super) const ONE_ROW: usize = 1;

/// Commands the scan does not run, each with the reason the report prints:
/// a silent skip is how a scan starts lying.
pub(super) const SKIP: [(&str, &str); 6] = [
    ("/update", "runs the automated self-update"),
    ("/rebuild", "rebuilds the binary and restarts the session"),
    (
        "/refresh",
        "mutates live Weles credentials other journeys depend on",
    ),
    ("/compact", "spawns a model turn (quota and minutes)"),
    ("/btw", "spawns a model turn (quota and minutes)"),
    ("/exit", "ends the session; jeden-exit covers it"),
];

/// Read-only subcommands from jeden's dispatcher (`rust/slash/mod.rs` status
/// and list arms, `rust/cli/billing.rs` BILLING_SLASH_HANDLERS). Pickers show
/// labels, not the commands behind them, so reading screens alone finds few.
pub(super) const SEED_SUBCOMMANDS: [&str; 15] = [
    "/billing policy get",
    "/subscriptions list",
    "/subscriptions status",
    "/plan status",
    "/goal status",
    "/loop status",
    "/fast status",
    "/advisor status",
    "/approval status",
    "/todo list",
    "/session list",
    "/memory status",
    "/collab status",
    "/roadmap list",
    "/tools --json",
];
