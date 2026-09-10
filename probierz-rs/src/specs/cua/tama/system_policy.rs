use std::collections::BTreeMap;
use std::time::Duration;

use crate::specs;

use crate::specs::cua::common;

pub fn run(context: &specs::Context) -> Result<(), String> {
    let executable = common::executable(context, "path to the Tama native application executable")?;
    let driver = common::driver(context)?;
    let environment = BTreeMap::from([("TAMA_TEST_IDENTITY".to_string(), "1".to_string())]);
    let app = driver.launch_process(&executable, &environment, &[])?;
    let result = (|| {
        driver.wait_for_text(
            app.pid,
            app.window_id,
            "AXStaticText = \"Snapshot validation\"",
            Duration::from_secs(30),
        )?;
        driver.select_sidebar_row(app.pid, app.window_id, 3)?;
        let validation = driver.wait_for_text(
            app.pid,
            app.window_id,
            "AXStaticText = \"Structurally valid\"",
            Duration::from_secs(30),
        )?;
        common::require_contains(
            &validation.tree,
            "AXStaticText = \"Snapshot validation\"",
            || "the Snapshot validation panel should be open".to_string(),
        )?;
        common::require_contains(&validation.tree, "AXStaticText = \"Status\"", || {
            "the snapshot structure status should render".to_string()
        })?;
        common::require_contains(
            &validation.tree,
            "AXStaticText = \"Structurally valid\"",
            || "the snapshot structure should render in a Valid state".to_string(),
        )?;
        common::require_contains(
            &validation.tree,
            "Tama validates snapshot structure.",
            || {
                "the rendered validation section should describe snapshot structure validation"
                    .to_string()
            },
        )?;
        common::require_absent(&validation.tree, "AXStaticText = \"Invalid\"", || {
            "the snapshot structure should not render an Invalid state".to_string()
        })
    })();
    driver.quit_app(app.pid);
    result
}
