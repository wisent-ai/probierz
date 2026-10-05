//! Waiting for a freshly launched CuaDriver daemon's socket on kernel events,
//! not a clock: the socket's directory changing, or the daemon exiting.

use crate::cua::*;

impl Driver {
    /// Block until the daemon `open` just launched creates `self.socket`, or
    /// fail with the daemon's own log when it exits before doing so. A daemon
    /// that neither creates its socket nor exits is a hung daemon, and the run
    /// shows it hung instead of guessing a limit.
    pub(crate) fn await_socket(&self, daemon_log: &Path) -> Result<(), String> {
        let not_created = || {
            let detail = fs::read_to_string(daemon_log).unwrap_or_default();
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
        let Some(pid) = self.daemon_pid()? else {
            return if self.socket.exists() {
                Ok(())
            } else {
                Err(not_created())
            };
        };
        let parent = self.socket.parent().unwrap_or_else(|| Path::new("."));
        let watch = kernel::Watch::new(parent, pid).map_err(|error| {
            format!(
                "cannot watch {} for CuaDriver's socket: {error}",
                parent.display()
            )
        })?;
        loop {
            // Checked after both events are registered, so a socket created
            // between the launch and the registration is not missed.
            if self.socket.exists() {
                return Ok(());
            }
            match watch
                .next()
                .map_err(|error| format!("watching CuaDriver's socket failed: {error}"))?
            {
                kernel::Event::DirectoryChanged => {}
                kernel::Event::DaemonExited => {
                    return if self.socket.exists() {
                        Ok(())
                    } else {
                        Err(not_created())
                    };
                }
            }
        }
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
        Ok(String::from_utf8_lossy(&output.stdout).trim().parse().ok())
    }
}

#[cfg(target_os = "macos")]
mod kernel {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    #[repr(C)]
    struct KEvent {
        ident: usize,
        filter: i16,
        flags: u16,
        fflags: u32,
        data: isize,
        udata: *mut std::ffi::c_void,
    }
    extern "C" {
        fn kqueue() -> i32;
        fn kevent(
            kq: i32,
            changelist: *const KEvent,
            nchanges: i32,
            eventlist: *mut KEvent,
            nevents: i32,
            wait_spec: *const std::ffi::c_void,
        ) -> i32;
        fn open(path: *const std::ffi::c_char, oflag: i32, ...) -> i32;
        fn close(fd: i32) -> i32;
    }
    const EVFILT_VNODE: i16 = -4;
    const EVFILT_PROC: i16 = -5;
    const EV_ADD: u16 = 0x1;
    const EV_CLEAR: u16 = 0x20;
    const NOTE_WRITE: u32 = 0x2;
    const NOTE_EXIT: u32 = 0x8000_0000;
    /// Open for event notification only: the descriptor reads nothing.
    const O_EVTONLY: i32 = 0x8000;
    /// `kevent` answers this for a pid that has already exited.
    const ESRCH: i32 = 3;

    pub(super) enum Event {
        DirectoryChanged,
        DaemonExited,
    }

    pub(super) struct Watch {
        kq: i32,
        directory: i32,
        exited_early: bool,
    }

    impl Watch {
        pub(super) fn new(directory: &Path, pid: u32) -> io::Result<Self> {
            let path = CString::new(directory.as_os_str().as_bytes())
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
            // SAFETY: a valid NUL-terminated path; a negative answer is the error.
            let directory = unsafe { open(path.as_ptr(), O_EVTONLY) };
            if directory < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: kqueue takes no arguments; a negative answer is its error.
            let kq = unsafe { kqueue() };
            if kq < 0 {
                let error = io::Error::last_os_error();
                // SAFETY: directory is the descriptor opened above.
                unsafe { close(directory) };
                return Err(error);
            }
            let mut watch = Watch {
                kq,
                directory,
                exited_early: false,
            };
            watch.register(directory as usize, EVFILT_VNODE, NOTE_WRITE)?;
            if let Err(error) = watch.register(pid as usize, EVFILT_PROC, NOTE_EXIT) {
                if error.raw_os_error() != Some(ESRCH) {
                    return Err(error);
                }
                watch.exited_early = true;
            }
            Ok(watch)
        }

        fn register(&self, ident: usize, filter: i16, fflags: u32) -> io::Result<()> {
            let change = KEvent {
                ident,
                filter,
                flags: EV_ADD | EV_CLEAR,
                fflags,
                data: 0,
                udata: std::ptr::null_mut(),
            };
            // SAFETY: one valid change and no event slots, so the call returns at once.
            let answered = unsafe {
                kevent(
                    self.kq,
                    &change,
                    1,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            };
            if answered < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// Block until the directory changes or the daemon exits.
        pub(super) fn next(&self) -> io::Result<Event> {
            if self.exited_early {
                return Ok(Event::DaemonExited);
            }
            let mut fired = KEvent {
                ident: 0,
                filter: 0,
                flags: 0,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            };
            // SAFETY: no changes and one writable event slot; a null wait spec
            // blocks until one registered event fires.
            let answered = unsafe {
                kevent(
                    self.kq,
                    std::ptr::null(),
                    0,
                    &mut fired,
                    1,
                    std::ptr::null(),
                )
            };
            if answered < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(if fired.filter == EVFILT_PROC {
                Event::DaemonExited
            } else {
                Event::DirectoryChanged
            })
        }
    }

    impl Drop for Watch {
        fn drop(&mut self) {
            // SAFETY: both are descriptors this watch opened and still owns.
            unsafe {
                close(self.kq);
                close(self.directory);
            }
        }
    }
}

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
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "CuaDriver's socket watch exists on macOS only",
            ))
        }

        pub(super) fn next(&self) -> io::Result<Event> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "CuaDriver's socket watch exists on macOS only",
            ))
        }
    }
}
