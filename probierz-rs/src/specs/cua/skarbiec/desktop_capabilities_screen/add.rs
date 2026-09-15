//! Adding a route without a reason, and the proof that nothing moved.
//!
//! The drawer is filled in completely except for the reason, its own
//! Add route button is pressed, and then three things must all hold:
//! the drawer still refuses, no route appears, and the real table, its
//! audit log and its backups are byte-for-byte what they were.

use super::*;

/// The drawer's own words when the reason is empty.
const EMPTY_REASON_REFUSAL: &str =
    "AXStaticText = \"A reason is required and is recorded with the change.\"";

/// The route the journey tries to add.
const CANDIDATE: [(&str, &str); 3] = [
    ("Resource", "https://probierz.example.com"),
    ("Item", "example-login"),
    ("Field", "password"),
];

/// The action bar and the drawer each render an Add route button, so
/// the drawer's own is the second one.
const EXPECTED_ADD_BUTTONS: usize = 2;

pub(crate) fn assert_add_refused(
    context: &specs::Context,
    driver: &cua::Driver,
    app: &cua::App,
    fixture: &Fixture,
    before: &TableBefore,
    window_id: u64,
) -> Result<(), String> {
    common::activate(
        context,
        driver,
        app.pid,
        window_id,
        "the Add route action",
        |element| cua::element_label(element) == "Add route" && common::is_button(element),
        |tree| tree.contains("AXStaticText = \"Add a route\""),
        STEP_TIMEOUT,
    )?;
    for (field, value) in CANDIDATE {
        common::type_field(context, driver, app.pid, window_id, field, value, true)?;
    }

    let filled = driver.snapshot(app.pid, window_id)?;
    common::dump_tree(context, "add-route-empty-reason", &filled.tree)?;
    if !filled.tree.contains(EMPTY_REASON_REFUSAL) {
        return Err("an empty reason should be refused in the drawer's own words".to_string());
    }
    if !filled.tree.contains("--reason '' --json") {
        return Err("the previewed command should show the empty reason it would carry".to_string());
    }
    common::capture(context, driver, app.pid, window_id, "add-route-refused")?;

    let press_refusal = press_drawer_add(driver, app, window_id, &filled)?;
    std::thread::sleep(REFUSAL_SETTLE);
    let after = driver.snapshot(app.pid, window_id)?.tree;
    common::dump_tree(context, "add-route-after-press", &after)?;
    if !after.contains(EMPTY_REASON_REFUSAL) {
        return Err(format!(
            "the drawer should still refuse after the action was invoked{}",
            press_refusal
                .map(|message| format!(" (press refused: {message})"))
                .unwrap_or_default()
        ));
    }
    if after.contains("Route added") {
        return Err("no route may be written without a reason".to_string());
    }
    assert_table_untouched(fixture, before)
}

/// Press the drawer's own Add route button — the lower of the two the
/// screen renders. Returns why the press was refused, when it was;
/// that is evidence, not a failure.
fn press_drawer_add(
    driver: &cua::Driver,
    app: &cua::App,
    window_id: u64,
    filled: &cua::Snapshot,
) -> Result<Option<String>, String> {
    let rendered = filled.tree.matches("AXButton (Add route)").count();
    if rendered < EXPECTED_ADD_BUTTONS {
        return Err(
            "the drawer should render its own Add route button beside the action bar's".to_string(),
        );
    }
    let state = driver.snapshot(app.pid, window_id)?;
    let named: Vec<&Value> = state
        .elements
        .iter()
        .filter(|element| cua::element_label(element) == "Add route" && common::is_button(element))
        .collect();
    if named.len() < rendered {
        return Ok(Some(
            "the drawer's Add route button exposes no press action while the reason is empty"
                .to_string(),
        ));
    }
    let lowest = named.into_iter().max_by(|left, right| {
        let y = |element: &Value| {
            cua::element_frame(element)
                .map(|frame| frame.y)
                .unwrap_or(0.0)
        };
        y(left).total_cmp(&y(right))
    });
    let Some(submit) = lowest else {
        return Ok(None);
    };
    match driver.click_element(app.pid, window_id, &state, submit) {
        Ok(_) => Ok(None),
        Err(error) => Ok(Some(error)),
    }
}

/// The refused add must have left the real table, its audit log and
/// its backups exactly as they were.
fn assert_table_untouched(fixture: &Fixture, before: &TableBefore) -> Result<(), String> {
    if fixture.fingerprint()? != before.fingerprint {
        return Err("the capability routes table must be untouched".to_string());
    }
    if fs::read(&fixture.routes).unwrap_or_default() != before.routes {
        return Err("the routes table content must be unchanged".to_string());
    }
    if fs::read(&fixture.routes_audit).unwrap_or_default() != before.audit {
        return Err("no audit line may be written for a refused route".to_string());
    }
    let new_backups: Vec<String> = fixture
        .backups()?
        .difference(&before.backups)
        .cloned()
        .collect();
    if !new_backups.is_empty() {
        return Err("a refused add must not publish a new table backup".to_string());
    }
    Ok(())
}
