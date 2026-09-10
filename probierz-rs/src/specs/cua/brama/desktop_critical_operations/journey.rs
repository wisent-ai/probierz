use super::*;
pub(crate) fn run_app(context: &specs::Context, executable: &Path, fixture: &Fixture) -> Result<(), String> {
    let driver = common::driver(context)?;
    let environment = BTreeMap::from([
        ("BRAMA_LOCAL_RUNTIME".to_string(), "1".to_string()),
        ("BRAMA_BASE_URL".to_string(), fixture.runtime_origin.clone()),
        (
            "BRAMA_DESKTOP_STATE_DIR".to_string(),
            fixture.state_root.to_string_lossy().into_owned(),
        ),
        (
            "BRAMA_DESKTOP_KEYCHAIN_NAMESPACE".to_string(),
            fixture.namespace.clone(),
        ),
    ]);
    let app = driver.launch_process(executable, &environment, &[])?;
    let result = (|| {
        driver.bring_to_front(app.pid, app.window_id)?;
        thread::sleep(Duration::from_millis(1500));
        let (shell_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Subscriptions",
            Duration::from_secs(60),
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            shell_window,
            "Subscriptions",
            |element| cua::element_label(element) == "Subscriptions" && common::is_button(element),
            |tree| tree.contains("Add local key"),
            Duration::from_secs(30),
        )?;
        let (subscriptions_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Add local key",
            Duration::from_secs(30),
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            subscriptions_window,
            "Add local key",
            |element| cua::element_label(element) == "Add local key" && common::is_button(element),
            |tree| tree.contains("Add a local provider key"),
            Duration::from_secs(15),
        )?;
        let (dialog_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Add a local provider key",
            Duration::from_secs(20),
        )?;
        common::type_field(
            context,
            &driver,
            app.pid,
            dialog_window,
            "Provider",
            fixture.provider,
            false,
        )?;
        common::type_field(
            context,
            &driver,
            app.pid,
            dialog_window,
            "API key or subscription credential",
            &fixture.first_key,
            false,
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            dialog_window,
            "Add key",
            |element| cua::element_label(element) == "Add key" && common::is_button(element),
            |tree| !tree.contains("Add a local provider key"),
            Duration::from_secs(30),
        )?;
        wait_until(
            || Ok(fixture.keychain_value().as_deref() == Some(&fixture.first_key)),
            "the exact first provider credential in Keychain",
        )?;
        let (subscriptions_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            fixture.provider,
            Duration::from_secs(30),
        )?;
        common::capture(
            context,
            &driver,
            app.pid,
            subscriptions_window,
            "provider-key-added",
        )?;

        common::activate(
            context,
            &driver,
            app.pid,
            subscriptions_window,
            fixture.provider,
            |element| {
                cua::element_label(element).starts_with(fixture.provider)
                    && common::is_button(element)
            },
            |tree| tree.contains("Replace this provider key"),
            Duration::from_secs(15),
        )?;
        let (inspector_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Replace this provider key",
            Duration::from_secs(20),
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            inspector_window,
            "Replace this provider key",
            |element| {
                cua::element_label(element).starts_with("Replace this provider key")
                    && common::is_button(element)
            },
            |tree| tree.contains("Replace a local provider key"),
            Duration::from_secs(15),
        )?;
        let (dialog_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Replace a local provider key",
            Duration::from_secs(20),
        )?;
        common::type_field(
            context,
            &driver,
            app.pid,
            dialog_window,
            "API key or subscription credential",
            &fixture.replacement_key,
            false,
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            dialog_window,
            "Replace credential",
            |element| {
                cua::element_label(element) == "Replace credential" && common::is_button(element)
            },
            |tree| !tree.contains("Replace a local provider key"),
            Duration::from_secs(30),
        )?;
        wait_until(
            || Ok(fixture.keychain_value().as_deref() == Some(&fixture.replacement_key)),
            "the exact replacement provider credential in Keychain",
        )?;
        let (subscriptions_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "openai is saved",
            Duration::from_secs(30),
        )?;
        common::capture(
            context,
            &driver,
            app.pid,
            subscriptions_window,
            "provider-key-replaced",
        )?;

        common::activate(
            context,
            &driver,
            app.pid,
            subscriptions_window,
            "Routing",
            |element| cua::element_label(element) == "Routing" && common::is_button(element),
            |tree| tree.contains("Add alias"),
            Duration::from_secs(30),
        )?;
        let (routing_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Add alias",
            Duration::from_secs(30),
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            routing_window,
            "Add alias",
            |element| cua::element_label(element) == "Add alias" && common::is_button(element),
            |tree| tree.contains("Create an alias"),
            Duration::from_secs(15),
        )?;
        let (dialog_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Create an alias",
            Duration::from_secs(20),
        )?;
        common::type_field(
            context,
            &driver,
            app.pid,
            dialog_window,
            "Alias",
            &fixture.alias,
            false,
        )?;
        common::type_field(
            context,
            &driver,
            app.pid,
            dialog_window,
            "Primary target",
            "openai/default",
            false,
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            dialog_window,
            "Create alias",
            |element| cua::element_label(element) == "Create alias" && common::is_button(element),
            |tree| !tree.contains("Create an alias"),
            Duration::from_secs(30),
        )?;
        wait_until(
            || Ok(fixture.route_target()?.as_deref() == Some("openai/default")),
            "the added route in routes.json",
        )?;
        let (routing_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            &fixture.alias,
            Duration::from_secs(30),
        )?;
        common::capture(
            context,
            &driver,
            app.pid,
            routing_window,
            "route-alias-added",
        )?;

        common::activate(
            context,
            &driver,
            app.pid,
            routing_window,
            &fixture.alias,
            |element| {
                cua::element_label(element).starts_with(&fixture.alias)
                    && common::is_button(element)
            },
            |tree| tree.contains("Review change"),
            Duration::from_secs(15),
        )?;
        let (inspector_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Review change",
            Duration::from_secs(20),
        )?;
        common::type_field(
            context,
            &driver,
            app.pid,
            inspector_window,
            "Primary target",
            "openai/fail",
            false,
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            inspector_window,
            "Review change",
            |element| {
                cua::element_label(element).starts_with("Review change")
                    && common::is_button(element)
            },
            |tree| tree.contains("Rewrite this route"),
            Duration::from_secs(15),
        )?;
        let (dialog_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Rewrite this route",
            Duration::from_secs(20),
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            dialog_window,
            "Rewrite the route",
            |element| {
                cua::element_label(element) == "Rewrite the route" && common::is_button(element)
            },
            |tree| !tree.contains("Rewrite this route"),
            Duration::from_secs(30),
        )?;
        wait_until(
            || Ok(fixture.route_target()?.as_deref() == Some("openai/fail")),
            "the replaced route in routes.json",
        )?;
        let (routing_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "openai/fail",
            Duration::from_secs(30),
        )?;
        common::capture(
            context,
            &driver,
            app.pid,
            routing_window,
            "route-alias-replaced",
        )?;

        common::activate(
            context,
            &driver,
            app.pid,
            routing_window,
            "Delete this alias",
            |element| {
                cua::element_label(element).starts_with("Delete this alias")
                    && common::is_button(element)
            },
            |tree| tree.contains("Delete this alias?"),
            Duration::from_secs(15),
        )?;
        let (dialog_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Delete this alias?",
            Duration::from_secs(20),
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            dialog_window,
            "Delete the alias",
            |element| {
                cua::element_label(element) == "Delete the alias" && common::is_button(element)
            },
            |tree| !tree.contains("Delete this alias?"),
            Duration::from_secs(30),
        )?;
        wait_until(
            || Ok(fixture.route_target()?.is_none()),
            "the alias deletion in routes.json",
        )?;
        let (routing_window, routing) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Add alias",
            Duration::from_secs(30),
        )?;
        if routing.tree.contains(&fixture.alias) {
            return Err("the deleted alias must be absent from the UI".to_string());
        }
        common::capture(
            context,
            &driver,
            app.pid,
            routing_window,
            "route-alias-deleted",
        )?;

        common::activate(
            context,
            &driver,
            app.pid,
            routing_window,
            "Subscriptions after route deletion",
            |element| cua::element_label(element) == "Subscriptions" && common::is_button(element),
            |tree| tree.contains(fixture.provider),
            Duration::from_secs(30),
        )?;
        let (subscriptions_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            fixture.provider,
            Duration::from_secs(30),
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            subscriptions_window,
            "saved provider",
            |element| {
                cua::element_label(element).starts_with(fixture.provider)
                    && common::is_button(element)
            },
            |tree| tree.contains("Remove this provider key"),
            Duration::from_secs(15),
        )?;
        let (inspector_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Remove this provider key",
            Duration::from_secs(20),
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            inspector_window,
            "Remove this provider key",
            |element| {
                cua::element_label(element).starts_with("Remove this provider key")
                    && common::is_button(element)
            },
            |tree| tree.contains("Remove the openai API key?"),
            Duration::from_secs(15),
        )?;
        let (dialog_window, _) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Remove the openai API key?",
            Duration::from_secs(20),
        )?;
        common::activate(
            context,
            &driver,
            app.pid,
            dialog_window,
            "Remove it",
            |element| cua::element_label(element) == "Remove it" && common::is_button(element),
            |tree| !tree.contains("Remove the openai API key?"),
            Duration::from_secs(30),
        )?;
        wait_until(
            || Ok(fixture.keychain_value().is_none()),
            "provider deletion from Keychain",
        )?;
        let (subscriptions_window, subscriptions) = common::wait_for_window_text(
            context,
            &driver,
            app.pid,
            "Add local key",
            Duration::from_secs(30),
        )?;
        if subscriptions.tree.contains(fixture.provider) {
            return Err("the deleted provider must be absent from the UI".to_string());
        }
        common::capture(
            context,
            &driver,
            app.pid,
            subscriptions_window,
            "provider-key-deleted",
        )?;
        Ok(())
    })();
    driver.quit_app(app.pid);
    result
}

pub fn run(context: &specs::Context) -> Result<(), String> {
    let executable =
        common::executable(context, "path to the Brama native application executable")?;
    let fixture = Fixture::new(context)?;
    fs::create_dir_all(&fixture.state_root)
        .map_err(|error| format!("{}: {error}", fixture.state_root.display()))?;
    fixture.delete_keychain();
    if fixture.keychain_value().is_some() {
        fixture.cleanup();
        return Err("the isolated provider must start absent from Keychain".to_string());
    }
    let result = run_app(context, &executable, &fixture);
    fixture.cleanup();
    result
}
