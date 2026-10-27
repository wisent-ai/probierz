//! Waiting for a freshly launched CuaDriver daemon's socket on kernel events,
//! not a clock: the socket's directory changing, or the daemon exiting.

use crate::cua::*;

#[cfg(target_os = "macos")]
mod kqueue;
#[cfg(target_os = "macos")]
use kqueue as kernel;

impl Driver {
    /// Block until the daemon `open` just launched creates `self.socket`, or
    /// fail with the daemon's own log when it exits before doing so. A daemon
    /// that neither creates its socket nor exits is a hung daemon, and the run
    /// shows it hung instead of guessing a limit.
    pub(crate) fn await_socket(&self, daemon_log: &Path) -> Result<(), String> {
        let socket_exists = || {
            self.socket.try_exists().map_err(|error| {
                format!(
                    "inspect CuaDriver socket {}: {error}",
                    self.socket.display()
                )
            })
        };
        let not_created = || {
            let detail = match fs::read_to_string(daemon_log) {
                Ok(detail) => detail,
                Err(error) => {
                    return format!(
                        "CuaDriver exited without creating {}; read daemon log {}: {error}",
                        self.socket.display(),
                        daemon_log.display()
                    )
                }
            };
            if detail.is_empty() {
                format!(
                    "CuaDriver exited without creating {}",
                    self.socket.display()
                )
            } else {
                format!(
                    "CuaDriver exited without creating {}:\n{detail}",
                    self.socket.display()
                )
            }
        };
        // The daemon is gone: its socket is there, or its log says why not.
        let exited = || -> Result<(), String> {
            if socket_exists()? {
                Ok(())
            } else {
                Err(not_created())
            }
        };
        let Some(pid) = self.daemon_pid()? else {
            return exited();
        };
        let parent = self.socket_directory()?;
        let watch = kernel::Watch::new(parent, pid).map_err(|error| {
            format!(
                "cannot watch {} for CuaDriver's socket: {error}",
                parent.display()
            )
        })?;
        loop {
            // Checked after both events are registered, so a socket created
            // between the launch and the registration is not missed.
            if socket_exists()? {
                return Ok(());
            }
            // Said on stderr at both ends, as every wait of Probierz is.
            let next = stado_wait::blocking(
                stado_wait::Kind::Process,
                format!("CuaDriver pid {pid} to create {}", self.socket.display()),
                parent.display(),
                || watch.next(),
            )
            .map_err(|error| format!("watching CuaDriver's socket failed: {error}"))?;
            match next {
                kernel::Event::DirectoryChanged => {}
                kernel::Event::DaemonExited => return exited(),
            }
        }
    }

    /// The directory that holds the driver's socket: the daemon's log is
    /// written and its socket awaited there.
    pub(crate) fn socket_directory(&self) -> Result<&Path, String> {
        crate::paths::directory_of(&self.socket)
            .map_err(|detail| format!("CuaDriver socket: {detail}"))
    }

    /// The pid of the `serve` process serving this socket: the newest process
    /// whose argv names it. `None` when it has already exited.
    fn daemon_pid(&self) -> Result<Option<u32>, String> {
        let output = run_to_exit(
            Command::new("pgrep")
                .arg("-n")
                .arg("-f")
                .arg(format!("serve --socket {}", self.socket.display())),
        )
        .map_err(|error| format!("cannot find the CuaDriver daemon's pid: {error}"))?;
        // Only no match is daemon absence; any other failure is pgrep's own.
        if output.status.code() == Some(PGREP_NO_MATCH) {
            return Ok(None);
        }
        if !output.status.success() {
            return Err(format!(
                "find CuaDriver daemon for {}: pgrep {}; {}",
                self.socket.display(),
                crate::failure::ended(&output.status),
                output_detail(&output)
            ));
        }
        let observed = String::from_utf8(output.stdout)
            .map_err(|error| format!("decode CuaDriver pgrep response: {error}"))?;
        observed.trim().parse::<u32>().map(Some).map_err(|error| {
            format!(
                "parse CuaDriver pid for {} from {observed:?}: {error}",
                self.socket.display()
            )
        })
    }
}

const PGREP_NO_MATCH: i32 = 1; // https://man.freebsd.org/cgi/man.cgi?query=pgrep ("1 No processes were matched.")

#[cfg(not(target_os = "macos"))]
mod kernel {
    use std::io;
    use std::path::Path;

    #[allow(dead_code)]
    pub(super) enum Event {
        DirectoryChanged,
        DaemonExited,
    }

    pub(super) struct Watch;

    impl Watch {
        /// CuaDriver is launched through macOS LaunchServices; no other
        /// system reaches this, and none is given a clock in its place.
        pub(super) fn new(_directory: &Path, _pid: u32) -> io::Result<Self> {
            Err(unsupported())
        }

        pub(super) fn next(&self) -> io::Result<Event> {
            Err(unsupported())
        }
    }

    fn unsupported() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "CuaDriver's socket watch exists on macOS only")
    }
}
