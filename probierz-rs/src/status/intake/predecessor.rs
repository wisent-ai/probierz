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

#[cfg(target_os = "macos")]
use std::time::Duration;

/// The one unit the fleet runs Probierz under, as the Stado catalog names it.
pub(crate) const DECLARED_UNIT: &str = "com.wisent.probierz";

/// The units whose work the declared intake does: the catalog's retired
/// units of Probierz, in the same order.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const PREDECESSORS: [&str; 2] = [
    "com.wisent.compute.service.probierz",
    "com.wisent.probierz-intake",
];

/// launchd's default `ExitTimeOut`: how long it gives a booted-out job before
/// it kills it, so no predecessor outlives a wait this long.
#[cfg(target_os = "macos")]
const LAUNCHD_EXIT_TIMEOUT: Duration = Duration::from_secs(20);

/// How often the wait asks whether the predecessor is gone.
#[cfg(target_os = "macos")]
const EXIT_POLL: Duration = Duration::from_millis(100);

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
/// no pid until launchd's restart binds the port. Binding waits until that
/// process is gone, at most as long as launchd waits before killing it.
#[cfg(target_os = "macos")]
fn await_exit(pid: i32) {
    use std::time::Instant;

    extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    let deadline = Instant::now() + LAUNCHD_EXIT_TIMEOUT;
    // SAFETY: signal 0 only asks whether the pid exists; nothing is delivered.
    while unsafe { kill(pid, 0) } == 0 && Instant::now() < deadline {
        std::thread::sleep(EXIT_POLL);
    }
}

#[cfg(not(target_os = "macos"))]
fn launchd(_predecessor: &str) {}
