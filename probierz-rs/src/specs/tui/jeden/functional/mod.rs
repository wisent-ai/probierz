//! What a jeden command does, not what it paints: mode toggles read back,
//! config writes reach disk, renames stick, checkpoints exist afterwards.
//! Every session runs on a sandbox home and a scratch workspace, so commands
//! with effects are exercised for real.

use std::cell::RefCell;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::specs::{self, tui::common};
use crate::tui::Terminal;

use super::views::discovery::{flattened, in_session};
use super::views::replays::submit;

pub(crate) mod files;
pub(crate) mod keys;
pub(crate) mod state;

/// How long a view gets to stop repainting after a command, in seconds.
const SETTLE_SECONDS: u64 = 15;

/// A marker no earlier run left behind.
fn marker(prefix: &str) -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    format!("{prefix}{stamp:x}")
}

/// The screen once two reads in a row agree: a mid-paint frame is not a view.
fn settled(app: &Terminal) -> Result<String, String> {
    let previous = RefCell::new(String::new());
    app.wait_until(
        "the screen to stop changing",
        Duration::from_secs(SETTLE_SECONDS),
        false,
        |screen| {
            let same = *previous.borrow() == screen;
            *previous.borrow_mut() = screen.to_string();
            same
        },
    )
    .map_err(|error| error.detail)
}

/// Run a command and return the settled screen, flattened.
fn run(app: &mut Terminal, command: &str) -> Result<String, String> {
    app.key("esc").map_err(|error| error.detail)?;
    submit(app, command)?;
    settled(app).map(|screen| flattened(&screen))
}

/// A journey body in a jeden session on a fresh scratch workspace; it gets
/// the terminal, the sandbox home and the workspace.
fn journey(
    context: &specs::Context,
    body: impl FnOnce(&mut Terminal, &Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    let cwd = common::scratch("probierz-functional")?;
    let outcome = in_session(context, &cwd, true, |app, home| body(app, home, &cwd));
    common::remove(&cwd);
    outcome
}
