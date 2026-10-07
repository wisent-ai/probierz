//! The units that ran Probierz's failure intake before the declared one,
//! retired when the declared intake starts.
//!
//! A host ran the intake as `com.wisent.probierz-intake` (the earlier Node
//! implementation, copied to `~/.local/share/probierz-intake` so a background
//! agent could read it) and later as `com.wisent.compute.service.probierz`, a
//! label Stado minted before the catalog named the unit. `probierz intake
//! serve` is the same listener in the product binary. Started by launchd as
//! the declared unit, it boots each predecessor out, waits for its process to
//! exit, and removes its launch agent before binding, so the port is free and
//! no login loads it again. Run by hand or by a test, it retires nothing.

/// The one unit the fleet runs Probierz under, as the Stado catalog names it.
pub(crate) const DECLARED_UNIT: &str = "com.wisent.probierz";

/// The units whose work the declared process does: the catalog's retired
/// units of Probierz, in the same order. The desktop driver's LaunchAgent is
/// among them: Probierz starts the CuaDriver app on demand from its own
/// process and owns its socket, so an agent that started the same app was a
/// second owner of that socket.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const PREDECESSORS: [&str; 4] = [
    "com.wisent.compute.service.probierz",
    "com.wisent.probierz-intake",
    "com.wisent.probierz-cua-driver",
    "com.wisent.compute.service.com.wisent.probierz-cua-driver",
];

pub(crate) fn retire() {
    let declared = std::env::var("XPC_SERVICE_NAME").is_ok_and(|label| label == DECLARED_UNIT);
    if declared {
        for predecessor in PREDECESSORS {
            launchd(predecessor);
        }
    }
}

#[cfg(target_os = "macos")]
fn launchd(predecessor: &str) {
    use std::path::PathBuf;
    use std::process::Command;

    extern "C" {
        fn getuid() -> u32;
    }
    // SAFETY: getuid has no preconditions and cannot fail.
    let target = format!("gui/{}/{predecessor}", unsafe { getuid() });
    let plist = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Library/LaunchAgents")
        .join(format!("{predecessor}.plist"));
    let printed = Command::new("/bin/launchctl")
        .arg("print")
        .arg(&target)
        .output()
        .ok()
        .filter(|output| output.status.success());
    if printed.is_none() && !plist.exists() {
        return;
    }
    let pid = printed.and_then(|output| running_pid(&String::from_utf8_lossy(&output.stdout)));
    let _ = Command::new("/bin/launchctl")
        .arg("bootout")
        .arg(&target)
        .output();
    if let Some(pid) = pid {
        await_exit(pid);
    }
    match std::fs::remove_file(&plist) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => eprintln!(
            "probierz intake: {predecessor} is stopped but {} stays: {error}",
            plist.display()
        ),
        _ => eprintln!(
            "probierz intake: retired {predecessor}: this process is the host's one Probierz intake"
        ),
    }
}

/// The pid `launchctl print` reports for a running job, on its `pid = N` line.
#[cfg(target_os = "macos")]
fn running_pid(printed: &str) -> Option<i32> {
    printed
        .lines()
        .find_map(|line| line.trim().strip_prefix("pid = "))
        .and_then(|pid| pid.parse().ok())
}

/// `launchctl bootout` can return while the Node intake still holds its
/// listener: the declared intake's next bind then fails with `Address already
/// in use`, exits 75, and `stado service ensure` reports the unit loaded with
/// no pid until launchd's restart binds the port. Binding waits for that
/// process's exit through the kernel's process-exit event (kqueue
/// `EVFILT_PROC`/`NOTE_EXIT`): no timer, no polling. A pid that is already
/// gone fails registration and is not waited for; a kqueue that cannot be
/// opened is reported and the bind then fails with its own error.
#[cfg(target_os = "macos")]
fn await_exit(pid: i32) {
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
            timeout: *const std::ffi::c_void,
        ) -> i32;
        fn close(fd: i32) -> i32;
    }
    const EVFILT_PROC: i16 = -5;
    const EV_ADD: u16 = 0x1;
    const NOTE_EXIT: u32 = 0x8000_0000;
    /// `kevent` answers this for a pid that has already exited.
    const ESRCH: i32 = 3;
    // SAFETY: kqueue takes no arguments; a negative answer is its error.
    let kq = unsafe { kqueue() };
    if kq < 0 {
        eprintln!(
            "probierz intake: cannot watch pid {pid} exit: kqueue failed: {}",
            std::io::Error::last_os_error()
        );
        return;
    }
    let change = KEvent {
        ident: pid as usize,
        filter: EVFILT_PROC,
        flags: EV_ADD,
        fflags: NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    let mut fired = KEvent {
        ident: 0,
        filter: 0,
        flags: 0,
        fflags: 0,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: one valid change and one writable event slot; a null timeout
    // blocks until the process exits. A pid already gone answers -1/ESRCH.
    let answered = unsafe { kevent(kq, &change, 1, &mut fired, 1, std::ptr::null()) };
    if answered < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ESRCH) {
            eprintln!("probierz intake: watching pid {pid} exit failed: {error}");
        }
    }
    // SAFETY: kq is the descriptor kqueue returned above.
    unsafe { close(kq) };
}

#[cfg(not(target_os = "macos"))]
fn launchd(_predecessor: &str) {}
