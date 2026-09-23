//! The launch agent that ran the Node failure intake, retired when the
//! declared intake starts.
//!
//! A host ran Probierz's failure intake as `com.wisent.probierz-intake`: the
//! earlier Node implementation, copied to `~/.local/share/probierz-intake` so
//! a background agent could read it. `probierz intake serve` is the same
//! listener in the product binary. Started by launchd as the declared unit,
//! it boots the Node intake out, waits for that process to exit, and removes
//! its launch agent before binding, so the port is free and no login loads the
//! copy again. Run by hand or by a test, it retires nothing.

#[cfg(target_os = "macos")]
use std::time::Duration;

/// The label the fleet runs the one Probierz process under.
pub(crate) const DECLARED_UNIT: &str = "com.wisent.compute.service.probierz";

/// The unit whose work the declared intake does.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const PREDECESSOR: &str = "com.wisent.probierz-intake";

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
        launchd();
    }
}

#[cfg(target_os = "macos")]
fn launchd() {
    use std::path::PathBuf;
    use std::process::Command;

    extern "C" {
        fn getuid() -> u32;
    }
    // SAFETY: getuid has no preconditions and cannot fail.
    let target = format!("gui/{}/{PREDECESSOR}", unsafe { getuid() });
    let plist = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Library/LaunchAgents")
        .join(format!("{PREDECESSOR}.plist"));
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
            "probierz intake: {PREDECESSOR} is stopped but {} stays: {error}",
            plist.display()
        ),
        _ => eprintln!(
            "probierz intake: retired {PREDECESSOR}: this process is the host's one Probierz intake"
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
/// listener: on lukasz-macbook on 2026-09-23 the declared intake's first bind
/// then failed with `Address already in use`, exited 75, and `stado service
/// ensure` reported the unit loaded with no pid until launchd's restart bound
/// the port. Binding waits until that process is gone, at most as long as
/// launchd waits before killing it.
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
fn launchd() {}
