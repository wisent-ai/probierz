//! The launch agent that ran the Node failure intake, retired when the
//! declared intake starts.
//!
//! A host ran Probierz's failure intake as `com.wisent.probierz-intake`: the
//! earlier Node implementation, copied to `~/.local/share/probierz-intake` so
//! a background agent could read it. `probierz intake serve` is the same
//! listener in the product binary. Started by launchd as the declared unit,
//! it boots the Node intake out and removes its launch agent before binding,
//! so the port is free and no login loads the copy again. Run by hand or by a
//! test, it retires nothing.

/// The label the fleet runs the one Probierz process under.
pub(crate) const DECLARED_UNIT: &str = "com.wisent.compute.service.probierz";

/// The unit whose work the declared intake does.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const PREDECESSOR: &str = "com.wisent.probierz-intake";

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
    let loaded = Command::new("/bin/launchctl")
        .arg("print")
        .arg(&target)
        .output()
        .is_ok_and(|output| output.status.success());
    if !loaded && !plist.exists() {
        return;
    }
    let _ = Command::new("/bin/launchctl")
        .arg("bootout")
        .arg(&target)
        .output();
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

#[cfg(not(target_os = "macos"))]
fn launchd() {}
