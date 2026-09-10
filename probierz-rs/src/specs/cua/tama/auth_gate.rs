use std::time::Duration;

use crate::{cua, specs};

use crate::specs::cua::common;

pub fn run(context: &specs::Context) -> Result<(), String> {
    let bundle_id = context
        .optional("CUA_BUNDLE_ID")
        .unwrap_or_else(|| "ai.wisent.tama.desktop".to_string());
    let driver = common::driver(context)?;
    let app = driver.launch_app(Some(&bundle_id), None, &[], false)?;
    let result = (|| {
        let gate = driver.wait_for_text(
            app.pid,
            app.window_id,
            "AXButton (Continue with GitHub)",
            Duration::from_secs(10),
        )?;
        common::require_regex(
            &gate.tree,
            r#"AXStaticText = "Tama" id=wisent\.auth\.screen"#,
            || format!("the authorization gate does not render Tama: {}", gate.tree),
        )?;
        common::require_regex(
            &gate.tree,
            r#"AXStaticText = "Sign in with your Wisent account" id=wisent\.auth\.screen"#,
            || {
                format!(
                    "the authorization gate does not explain the Wisent sign-in: {}",
                    gate.tree
                )
            },
        )?;
        for pattern in [
            r#"AXTextField id=wisent\.auth\.screen"#,
            r#"AXButton \(Send one-time code\) id=wisent\.auth\.screen"#,
            r#"AXButton \(Continue with Google\) id=wisent\.auth\.screen"#,
            r#"AXButton \(Continue with GitHub\) id=wisent\.auth\.screen"#,
        ] {
            common::require_regex(&gate.tree, pattern, || {
                format!("the authorization gate is incomplete: {}", gate.tree)
            })?;
        }

        let input = driver.wait_for_text(
            app.pid,
            app.window_id,
            "AXTextField id=wisent.auth.screen",
            Duration::from_secs(10),
        )?;
        let index = cua::element_index_of(&input.tree, "AXTextField id=wisent.auth.screen")?;
        let element = input.element_by_index(index).ok_or_else(|| {
            "no indexed element matching \"AXTextField id=wisent.auth.screen\" in tree".to_string()
        })?;
        driver.type_text(
            app.pid,
            app.window_id,
            &input,
            element,
            "auth-gate@example.com",
            false,
        )?;

        let typed = driver.snapshot(app.pid, app.window_id)?.tree;
        common::require_regex(
            &typed,
            r#"AXTextField(?: = "auth-gate@example\.com")? id=wisent\.auth\.screen"#,
            || format!("the email field disappeared after typing: {typed}"),
        )?;
        common::require_contains(&typed, "auth-gate@example.com", || {
            "the email field should expose the typed email in the accessibility tree".to_string()
        })
    })();
    driver.quit_app(app.pid);
    result
}
