//! The Accessibility grant the `desktop:cua` preflight reports. The desktop
//! journeys themselves are driven by the harness's `cua-driver` packages; the
//! binary only answers whether the program running probierz holds the grant
//! a launched app's windows are observed with.

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
}

/// Whether this process may observe another app's windows (macOS
/// Accessibility trust); no other platform grants it.
pub(crate) fn accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    // SAFETY: a plain query of this process's Accessibility trust.
    return unsafe { AXIsProcessTrusted() != 0 };
    #[cfg(not(target_os = "macos"))]
    return false;
}
