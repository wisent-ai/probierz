//! The inspector for one pooled subscription, and the refresh it
//! refuses without a reason.
//!
//! The refusal is the point: the dialog renders the confirm button,
//! the journey presses it, and the screen must still refuse — with no
//! CLI invocation behind it. A refused action that quietly ran would
//! show up here as a new line in the fixture's invocation log.

use super::*;

/// Fields the inspector must render for a pooled subscription.
const INSPECTOR_FIELDS: [&str; 6] = [
    "IDENTITY",
    "PROVIDER",
    "SUBSCRIPTION ID",
    "STATE",
    "EXPIRY",
    "REFRESH",
];

/// What the inspector must say, beyond its field labels.
const INSPECTOR_TEXTS: [(&str, &str); 3] = [
    (
        "sub-openai-1c04",
        "the inspector should name the pooled subscription it is describing",
    ),
    (
        "Never available here",
        "the inspector should state what this screen never shows",
    ),
    (
        "Reading the credential value behind a pooled subscription",
        "the inspector should name the credential value as unavailable",
    ),
];

/// The dialog's own words when the reason is empty.
const EMPTY_REASON_REFUSAL: &str =
    "AXStaticText = \"A reason is required. The command refuses without one.\"";

/// Open the inspector for the burnt openai subscription and read it.
pub(crate) fn open_inspector(
    context: &specs::Context,
    driver: &cua::Driver,
    app: &cua::App,
    window_id: u64,
) -> Result<u64, String> {
    common::activate(
        context,
        driver,
        app.pid,
        window_id,
        "the openai row",
        |element| cua::element_label(element).starts_with("openai, ") && common::is_button(element),
        |tree| tree.contains("POOLED SUBSCRIPTION"),
        STEP_TIMEOUT,
    )?;
    let (inspector_window, inspector) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "POOLED SUBSCRIPTION",
        SCREEN_TIMEOUT,
    )?;
    common::dump_tree(context, "pool-inspector", &inspector.tree)?;
    for label in INSPECTOR_FIELDS {
        if !inspector.tree.contains(label) {
            return Err(format!("the inspector should render its {label} field"));
        }
    }
    for (needle, message) in INSPECTOR_TEXTS {
        if !inspector.tree.contains(needle) {
            return Err(message.to_string());
        }
    }
    assert_no_secret(&inspector.tree, "the inspector")?;
    Ok(inspector_window)
}

/// Ask for a refresh with no reason, press the confirm button anyway,
/// and require that the screen still refuses and the CLI was never
/// asked.
pub(crate) fn assert_refresh_refused(
    context: &specs::Context,
    driver: &cua::Driver,
    app: &cua::App,
    fixture: &Fixture,
    window_id: u64,
    inspector_window: u64,
) -> Result<(), String> {
    let before = fixture.invocations()?;
    common::activate(
        context,
        driver,
        app.pid,
        inspector_window,
        "the Refresh openai action",
        |element| {
            cua::element_label(element).starts_with("Refresh openai") && common::is_button(element)
        },
        |tree| tree.contains("Refresh the openai subscription pool?"),
        DIALOG_TIMEOUT,
    )?;
    let (dialog_window, dialog) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "AXStaticText = \"Refresh the openai subscription pool?\"",
        SCREEN_TIMEOUT,
    )?;
    common::dump_tree(context, "refresh-empty-reason", &dialog.tree)?;
    if !dialog.tree.contains(EMPTY_REASON_REFUSAL) {
        return Err("an empty reason should be refused in the dialog's own words".to_string());
    }
    if !dialog
        .tree
        .contains("brama subscription refresh openai --reason '' --json")
    {
        return Err("the previewed command should show the empty reason it would carry".to_string());
    }
    common::capture(context, driver, app.pid, dialog_window, "refresh-refused")?;

    let press_refusal = press_confirm(driver, app, dialog_window, &dialog)?;
    std::thread::sleep(REFUSAL_SETTLE);
    let after_press = driver.snapshot(app.pid, dialog_window)?.tree;
    common::dump_tree(context, "refresh-after-press", &after_press)?;
    if !after_press.contains(EMPTY_REASON_REFUSAL) {
        return Err(format!(
            "the dialog should still refuse after the action was invoked{}",
            press_refusal
                .map(|message| format!(" (press refused: {message})"))
                .unwrap_or_default()
        ));
    }

    assert_cli_untouched(fixture, &before)?;
    let shell_after = driver.snapshot(app.pid, window_id)?.tree;
    if shell_after.contains("AXStaticText = \"Refreshing openai credentials.") {
        return Err("no refresh may be in flight".to_string());
    }
    Ok(())
}

/// Press the confirm button the dialog renders. Returns why the press
/// itself was refused, when it was — that is part of the evidence, not
/// a failure.
fn press_confirm(
    driver: &cua::Driver,
    app: &cua::App,
    dialog_window: u64,
    dialog: &cua::Snapshot,
) -> Result<Option<String>, String> {
    if !dialog.tree.contains("AXButton (Refresh it)") {
        return Err("the dialog should render the confirm button it is refusing to run".to_string());
    }
    let state = driver.snapshot(app.pid, dialog_window)?;
    let confirms: Vec<&Value> = state
        .elements
        .iter()
        .filter(|element| cua::element_label(element) == "Refresh it" && common::is_button(element))
        .collect();
    let mut refusal = confirms.is_empty().then(|| {
        "the Refresh it button exposes no press action while the reason is empty".to_string()
    });
    if let Some(confirm) = confirms.first() {
        if let Err(error) = driver.click_element(app.pid, dialog_window, &state, confirm) {
            refusal = Some(error);
        }
    }
    Ok(refusal)
}

/// The refused refresh must have added no invocation, the CLI must
/// never have been asked to refresh, and the pool on screen must still
/// have come from a real read.
fn assert_cli_untouched(fixture: &Fixture, before: &[String]) -> Result<(), String> {
    let after = fixture.invocations()?;
    if after != before {
        return Err("a refused refresh must not invoke the Brama CLI".to_string());
    }
    if after
        .iter()
        .any(|line| line.starts_with("subscription refresh"))
    {
        return Err(format!(
            "the CLI must never be asked to refresh a pool here: {}",
            after.join(" | ")
        ));
    }
    if !after
        .iter()
        .any(|line| line.starts_with("subscriptions list"))
    {
        return Err("the pool on screen must come from a real CLI read".to_string());
    }
    Ok(())
}
