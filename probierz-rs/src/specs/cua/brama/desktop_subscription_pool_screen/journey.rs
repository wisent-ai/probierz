//! The journey: build the ledger, launch Brama Desktop against it,
//! open Subscription Pool, read the table, open one subscription, and
//! watch a refresh be refused without a reason.

use super::*;

pub fn run(context: &specs::Context) -> Result<(), String> {
    let executable =
        common::executable(context, "path to the Brama native application executable")?;
    let fixture = Fixture::new(&executable)?;
    fixture.build()?;

    let driver = common::driver(context)?;
    let app = driver.launch_process(&executable, &fixture.environment(), &[])?;
    let result = (|| {
        driver.bring_to_front(app.pid, app.window_id)?;
        std::thread::sleep(LAUNCH_SETTLE);

        let window_id = open_pool(context, &driver, &app)?;
        let inspector_window = open_inspector(context, &driver, &app, window_id)?;
        assert_refresh_refused(
            context,
            &driver,
            &app,
            &fixture,
            window_id,
            inspector_window,
        )
    })();
    driver.quit_app(app.pid);
    result
}

/// Open the Subscription Pool destination and read the loaded table.
fn open_pool(
    context: &specs::Context,
    driver: &cua::Driver,
    app: &cua::App,
) -> Result<u64, String> {
    let (shell_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "Subscription Pool",
        WINDOW_TIMEOUT,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        shell_window,
        "the Subscription Pool destination",
        |element| cua::element_label(element) == "Subscription Pool" && common::is_button(element),
        |tree| tree.contains("AXStaticText = \"LAST REDEEM ERROR\""),
        DIALOG_TIMEOUT,
    )?;
    let (window_id, loaded) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "AXStaticText = \"LAST REDEEM ERROR\"",
        WINDOW_TIMEOUT,
    )?;
    common::dump_tree(context, "pool-loaded", &loaded.tree)?;
    common::capture(context, driver, app.pid, window_id, "pool-loaded")?;
    assert_loaded_pool(&loaded)?;
    Ok(window_id)
}
