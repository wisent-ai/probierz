//! The Routing half of the journey: creating an alias, rewriting its
//! target, and deleting it — each confirmed in the real `routes.json`,
//! not only on screen.

use super::*;

/// The target the alias is created with, and the one it is rewritten
/// to. The second deliberately names a route that fails, because the
/// journey checks the product writes what it was told rather than what
/// it thinks will work.
const FIRST_TARGET: &str = "openai/default";
const REWRITTEN_TARGET: &str = "openai/fail";

pub(crate) fn add_alias(
    context: &specs::Context,
    driver: &cua::Driver,
    app: &cua::App,
    fixture: &Fixture,
    subscriptions_window: u64,
) -> Result<u64, String> {
    common::activate(
        context,
        driver,
        app.pid,
        subscriptions_window,
        "Routing",
        |element| cua::element_label(element) == "Routing" && common::is_button(element),
        |tree| tree.contains("Add alias"),
        SCREEN_TIMEOUT,
    )?;
    let (routing_window, _) =
        common::wait_for_window_text(context, driver, app.pid, "Add alias", SCREEN_TIMEOUT)?;
    common::activate(
        context,
        driver,
        app.pid,
        routing_window,
        "Add alias",
        |element| cua::element_label(element) == "Add alias" && common::is_button(element),
        |tree| tree.contains("Create an alias"),
        STEP_TIMEOUT,
    )?;
    let (dialog_window, _) =
        common::wait_for_window_text(context, driver, app.pid, "Create an alias", DIALOG_TIMEOUT)?;
    common::type_field(
        context,
        driver,
        app.pid,
        dialog_window,
        "Alias",
        &fixture.alias,
        false,
    )?;
    common::type_field(
        context,
        driver,
        app.pid,
        dialog_window,
        "Primary target",
        FIRST_TARGET,
        false,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        dialog_window,
        "Create alias",
        |element| cua::element_label(element) == "Create alias" && common::is_button(element),
        |tree| !tree.contains("Create an alias"),
        SCREEN_TIMEOUT,
    )?;
    wait_until(
        || Ok(fixture.route_target()?.as_deref() == Some(FIRST_TARGET)),
        "the added route in routes.json",
    )?;
    let (routing_window, _) =
        common::wait_for_window_text(context, driver, app.pid, &fixture.alias, SCREEN_TIMEOUT)?;
    common::capture(
        context,
        driver,
        app.pid,
        routing_window,
        "route-alias-added",
    )?;
    Ok(routing_window)
}

pub(crate) fn replace_alias(
    context: &specs::Context,
    driver: &cua::Driver,
    app: &cua::App,
    fixture: &Fixture,
    routing_window: u64,
) -> Result<u64, String> {
    common::activate(
        context,
        driver,
        app.pid,
        routing_window,
        &fixture.alias,
        |element| {
            cua::element_label(element).starts_with(&fixture.alias) && common::is_button(element)
        },
        |tree| tree.contains("Review change"),
        STEP_TIMEOUT,
    )?;
    let (inspector_window, _) =
        common::wait_for_window_text(context, driver, app.pid, "Review change", DIALOG_TIMEOUT)?;
    common::type_field(
        context,
        driver,
        app.pid,
        inspector_window,
        "Primary target",
        REWRITTEN_TARGET,
        false,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        inspector_window,
        "Review change",
        |element| {
            cua::element_label(element).starts_with("Review change") && common::is_button(element)
        },
        |tree| tree.contains("Rewrite this route"),
        STEP_TIMEOUT,
    )?;
    let (dialog_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "Rewrite this route",
        DIALOG_TIMEOUT,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        dialog_window,
        "Rewrite the route",
        |element| cua::element_label(element) == "Rewrite the route" && common::is_button(element),
        |tree| !tree.contains("Rewrite this route"),
        SCREEN_TIMEOUT,
    )?;
    wait_until(
        || Ok(fixture.route_target()?.as_deref() == Some(REWRITTEN_TARGET)),
        "the replaced route in routes.json",
    )?;
    let (routing_window, _) =
        common::wait_for_window_text(context, driver, app.pid, REWRITTEN_TARGET, SCREEN_TIMEOUT)?;
    common::capture(
        context,
        driver,
        app.pid,
        routing_window,
        "route-alias-replaced",
    )?;
    Ok(routing_window)
}

pub(crate) fn delete_alias(
    context: &specs::Context,
    driver: &cua::Driver,
    app: &cua::App,
    fixture: &Fixture,
    routing_window: u64,
) -> Result<u64, String> {
    common::activate(
        context,
        driver,
        app.pid,
        routing_window,
        "Delete this alias",
        |element| {
            cua::element_label(element).starts_with("Delete this alias")
                && common::is_button(element)
        },
        |tree| tree.contains("Delete this alias?"),
        STEP_TIMEOUT,
    )?;
    let (dialog_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "Delete this alias?",
        DIALOG_TIMEOUT,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        dialog_window,
        "Delete the alias",
        |element| cua::element_label(element) == "Delete the alias" && common::is_button(element),
        |tree| !tree.contains("Delete this alias?"),
        SCREEN_TIMEOUT,
    )?;
    wait_until(
        || Ok(fixture.route_target()?.is_none()),
        "the alias deletion in routes.json",
    )?;
    let (routing_window, routing) =
        common::wait_for_window_text(context, driver, app.pid, "Add alias", SCREEN_TIMEOUT)?;
    if routing.tree.contains(&fixture.alias) {
        return Err("the deleted alias must be absent from the UI".to_string());
    }
    common::capture(
        context,
        driver,
        app.pid,
        routing_window,
        "route-alias-deleted",
    )?;
    Ok(routing_window)
}
