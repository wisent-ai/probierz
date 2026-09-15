//! The journey itself: launch Brama's native application against the
//! isolated fixture, then perform the four critical operations a person
//! would perform — add a provider key, replace it, manage a route
//! alias, and remove the key — in that order, through its own windows.

use super::*;

pub(crate) fn run_app(
    context: &specs::Context,
    executable: &Path,
    fixture: &Fixture,
) -> Result<(), String> {
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
        thread::sleep(LAUNCH_SETTLE);

        let subscriptions = subscriptions::add_provider_key(context, &driver, &app, fixture)?;
        let subscriptions =
            subscriptions::replace_provider_key(context, &driver, &app, fixture, subscriptions)?;

        let routing = routing::add_alias(context, &driver, &app, fixture, subscriptions)?;
        let routing = routing::replace_alias(context, &driver, &app, fixture, routing)?;
        let routing = routing::delete_alias(context, &driver, &app, fixture, routing)?;

        subscriptions::delete_provider_key(context, &driver, &app, fixture, routing)
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
