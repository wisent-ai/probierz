//! Reading the Services screen: opening it, recognising a convergence
//! receipt, and refusing a screen that shows what it must not.

use super::*;

/// How long Services may take to render the real host-wide report. The
/// report is a live GET against the fixture's registry.
pub(crate) const SERVICES_TIMEOUT: Duration = Duration::from_secs(180);

/// Poll interval while Services is still loading.
const SERVICES_POLL: Duration = Duration::from_millis(500);

/// How much of the last screen a timeout quotes, so the failure says
/// what the journey actually saw.
const TAIL_CHARACTERS: usize = 2500;

/// Screens that mean the journey left its documented local path: an
/// account operation, or a source it cannot read.
pub(crate) const OFF_PATH_SCREENS: [&str; 5] = [
    "Connect to Stado",
    "This source cannot be read",
    "Sign In",
    "Continue with",
    "Enter your email",
];

/// Fail when the screen shows something the journey forbids.
pub(crate) fn absent(tree: &str, needles: &[&str], why: &str) -> Result<(), String> {
    for needle in needles {
        if tree.contains(needle) {
            return Err(format!("{why}: the screen shows {needle:?}"));
        }
    }
    Ok(())
}

/// A convergence receipt is on screen once it names itself, carries a
/// non-zero exit, names the service, and reports a failed status.
pub(crate) fn receipt_ready(tree: &str) -> bool {
    tree.contains("Convergence receipt")
        && Regex::new(r"exit [1-9][0-9]*").unwrap().is_match(tree)
        && tree.contains("skarbiec")
        && Regex::new(r#""status"\s*:\s*"failed""#)
            .unwrap()
            .is_match(tree)
}

/// Open Services and wait until the real host-wide report names the
/// fixture's target. Retries the screen's own Retry button, and
/// refuses the moment the client asks for an account.
pub(crate) fn open_services(
    driver: &crate::cua::Driver,
    app: &App,
    target: &str,
) -> Result<console::View, String> {
    console::click(driver, app.pid, app.window_id, "Services")?;
    let deadline = Instant::now() + SERVICES_TIMEOUT;
    loop {
        let view = console::read_window(driver, app.pid, app.window_id)?;
        if view.tree.contains(target) && Regex::new(r"(?i)skarbiec").unwrap().is_match(&view.tree) {
            return Ok(view);
        }
        if Regex::new(r"Sign In|Continue with|Enter your email|Connect to Stado")
            .unwrap()
            .is_match(&view.tree)
        {
            return Err("The dedicated local registry API client unexpectedly requested an account sign-in; this journey performs no Wisent account operation or provider flow.".into());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Services never rendered the real host-wide convergence report; last tree: {}",
                crate::specs::cua::common::tail(&view.tree, TAIL_CHARACTERS)
            ));
        }
        if view.tree.contains("Retry") {
            console::click(driver, app.pid, app.window_id, "Retry")?;
        }
        thread::sleep(SERVICES_POLL);
    }
}
