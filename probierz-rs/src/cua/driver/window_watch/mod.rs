//! Waiting for a launched app's first window on events, not a clock: the
//! app's own Accessibility notification that it created a window, or the
//! kernel's notice that its process exited.
//!
//! The window itself is still read from CuaDriver, so the `window_id` a run
//! acts on is the driver's. A notification for a window the driver does not
//! list yet is followed by the next one; an app that neither opens a window
//! nor exits shows as hung instead of being cut off at a guessed limit.

use crate::cua::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as events;

/// Whether this process may observe another app's windows; the preflight
/// reports it, because without it a launched app's window cannot be awaited.
pub(crate) fn accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    return macos::trusted();
    #[cfg(not(target_os = "macos"))]
    return false;
}

impl Driver {
    pub(crate) fn wait_for_window(&self, pid: u32) -> Result<App, String> {
        let watch = events::Watch::new(pid)?;
        loop {
            // Checked after both sources are registered, so a window opened
            // between the launch and the registration is not missed.
            if let Some(window) = self.find_window(pid)? {
                if let Some(window_id) = window.get("window_id").and_then(Value::as_u64) {
                    return Ok(App { pid, window_id });
                }
            }
            match watch.next()? {
                events::Event::WindowCreated => {}
                events::Event::Exited => {
                    return Err(format!("pid {pid} exited before it opened a window"));
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod events {
    pub(super) enum Event {
        #[allow(dead_code)]
        WindowCreated,
        #[allow(dead_code)]
        Exited,
    }

    pub(super) struct Watch;

    impl Watch {
        /// CuaDriver drives macOS apps; no other system reaches this, and
        /// none is given a clock in its place.
        pub(super) fn new(_pid: u32) -> Result<Self, String> {
            Err("waiting for an app's window exists on macOS only".to_string())
        }

        pub(super) fn next(&self) -> Result<Event, String> {
            Err("waiting for an app's window exists on macOS only".to_string())
        }
    }
}
