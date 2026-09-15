//! The Subscriptions half of the journey: adding a local provider key,
//! replacing it, and removing it — each confirmed in the real Keychain,
//! not only on screen.

use super::*;

pub(crate) fn add_provider_key(
    context: &specs::Context,
    driver: &cua::Driver,
    app: &cua::App,
    fixture: &Fixture,
) -> Result<u64, String> {
    let (shell_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "Subscriptions",
        WINDOW_TIMEOUT,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        shell_window,
        "Subscriptions",
        |element| cua::element_label(element) == "Subscriptions" && common::is_button(element),
        |tree| tree.contains("Add local key"),
        SCREEN_TIMEOUT,
    )?;
    let (subscriptions_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "Add local key",
        SCREEN_TIMEOUT,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        subscriptions_window,
        "Add local key",
        |element| cua::element_label(element) == "Add local key" && common::is_button(element),
        |tree| tree.contains("Add a local provider key"),
        STEP_TIMEOUT,
    )?;
    let (dialog_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "Add a local provider key",
        DIALOG_TIMEOUT,
    )?;
    common::type_field(
        context,
        driver,
        app.pid,
        dialog_window,
        "Provider",
        fixture.provider,
        false,
    )?;
    common::type_field(
        context,
        driver,
        app.pid,
        dialog_window,
        "API key or subscription credential",
        &fixture.first_key,
        false,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        dialog_window,
        "Add key",
        |element| cua::element_label(element) == "Add key" && common::is_button(element),
        |tree| !tree.contains("Add a local provider key"),
        SCREEN_TIMEOUT,
    )?;
    wait_until(
        || Ok(fixture.keychain_value().as_deref() == Some(&fixture.first_key)),
        "the exact first provider credential in Keychain",
    )?;
    let (subscriptions_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        fixture.provider,
        SCREEN_TIMEOUT,
    )?;
    common::capture(
        context,
        driver,
        app.pid,
        subscriptions_window,
        "provider-key-added",
    )?;
    Ok(subscriptions_window)
}

pub(crate) fn replace_provider_key(
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
        fixture.provider,
        |element| {
            cua::element_label(element).starts_with(fixture.provider) && common::is_button(element)
        },
        |tree| tree.contains("Replace this provider key"),
        STEP_TIMEOUT,
    )?;
    let (inspector_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "Replace this provider key",
        DIALOG_TIMEOUT,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        inspector_window,
        "Replace this provider key",
        |element| {
            cua::element_label(element).starts_with("Replace this provider key")
                && common::is_button(element)
        },
        |tree| tree.contains("Replace a local provider key"),
        STEP_TIMEOUT,
    )?;
    let (dialog_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "Replace a local provider key",
        DIALOG_TIMEOUT,
    )?;
    common::type_field(
        context,
        driver,
        app.pid,
        dialog_window,
        "API key or subscription credential",
        &fixture.replacement_key,
        false,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        dialog_window,
        "Replace credential",
        |element| cua::element_label(element) == "Replace credential" && common::is_button(element),
        |tree| !tree.contains("Replace a local provider key"),
        SCREEN_TIMEOUT,
    )?;
    wait_until(
        || Ok(fixture.keychain_value().as_deref() == Some(&fixture.replacement_key)),
        "the exact replacement provider credential in Keychain",
    )?;
    let (subscriptions_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "openai is saved",
        SCREEN_TIMEOUT,
    )?;
    common::capture(
        context,
        driver,
        app.pid,
        subscriptions_window,
        "provider-key-replaced",
    )?;
    Ok(subscriptions_window)
}

/// Come back from Routing, remove the provider key, and check both
/// Keychain and the screen agree it is gone.
pub(crate) fn delete_provider_key(
    context: &specs::Context,
    driver: &cua::Driver,
    app: &cua::App,
    fixture: &Fixture,
    from_window: u64,
) -> Result<(), String> {
    common::activate(
        context,
        driver,
        app.pid,
        from_window,
        "Subscriptions after route deletion",
        |element| cua::element_label(element) == "Subscriptions" && common::is_button(element),
        |tree| tree.contains(fixture.provider),
        SCREEN_TIMEOUT,
    )?;
    let (subscriptions_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        fixture.provider,
        SCREEN_TIMEOUT,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        subscriptions_window,
        "saved provider",
        |element| {
            cua::element_label(element).starts_with(fixture.provider) && common::is_button(element)
        },
        |tree| tree.contains("Remove this provider key"),
        STEP_TIMEOUT,
    )?;
    let (inspector_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "Remove this provider key",
        DIALOG_TIMEOUT,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        inspector_window,
        "Remove this provider key",
        |element| {
            cua::element_label(element).starts_with("Remove this provider key")
                && common::is_button(element)
        },
        |tree| tree.contains("Remove the openai API key?"),
        STEP_TIMEOUT,
    )?;
    let (dialog_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "Remove the openai API key?",
        DIALOG_TIMEOUT,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        dialog_window,
        "Remove it",
        |element| cua::element_label(element) == "Remove it" && common::is_button(element),
        |tree| !tree.contains("Remove the openai API key?"),
        SCREEN_TIMEOUT,
    )?;
    wait_until(
        || Ok(fixture.keychain_value().is_none()),
        "provider deletion from Keychain",
    )?;
    let (subscriptions_window, subscriptions) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "Add local key",
        SCREEN_TIMEOUT,
    )?;
    if subscriptions.tree.contains(fixture.provider) {
        return Err("the deleted provider must be absent from the UI".to_string());
    }
    common::capture(
        context,
        driver,
        app.pid,
        subscriptions_window,
        "provider-key-deleted",
    )
    .map(|_| ())
}
