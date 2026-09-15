//! The journey: build the real vault and route table, launch Skarbiec
//! Desktop against them, open Capabilities, read the table, and watch
//! an add be refused without a reason.

use super::*;

pub fn run(context: &specs::Context) -> Result<(), String> {
    let executable = common::executable(
        context,
        "path to the Skarbiec native application executable",
    )?;
    let fixture = Fixture::new(context)?;
    fixture.build()?;
    let before = TableBefore::read(&fixture)?;

    let driver = common::driver(context)?;
    let app = driver.launch_process(&executable, &fixture.environment(), &[])?;
    let result = (|| {
        driver.bring_to_front(app.pid, app.window_id)?;
        std::thread::sleep(LAUNCH_SETTLE);

        let window_id = open_capabilities(context, &driver, &app)?;
        assert_add_refused(context, &driver, &app, &fixture, &before, window_id)
    })();
    driver.quit_app(app.pid);
    result
}

/// Open the Capabilities destination and read the verified table.
fn open_capabilities(
    context: &specs::Context,
    driver: &cua::Driver,
    app: &cua::App,
) -> Result<u64, String> {
    let (shell_window, _) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "AXButton (Capabilities)",
        WINDOW_TIMEOUT,
    )?;
    common::activate(
        context,
        driver,
        app.pid,
        shell_window,
        "the Capabilities destination",
        |element| cua::element_label(element) == "Capabilities" && common::is_button(element),
        |tree| tree.contains("AXButton (Read routes)"),
        STEP_TIMEOUT,
    )?;
    let (window_id, loaded) = common::wait_for_window_text(
        context,
        driver,
        app.pid,
        "AXStaticText = \"https://login.example.com\"",
        VERIFIED_TABLE_TIMEOUT,
    )?;
    common::dump_tree(context, "capabilities-loaded", &loaded.tree)?;
    common::capture(context, driver, app.pid, window_id, "capabilities-loaded")?;
    assert_loaded_routes(&loaded)?;
    Ok(window_id)
}
