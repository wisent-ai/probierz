//! The journey: select the dedicated host in Stado Desktop, read Apple
//! code-capture readiness, prepare it, and check the product reported
//! what it actually did — all without changing CuaDriver's own
//! permission state.

use super::*;

/// Slug every screenshot from this journey is filed under.
const SLUG: &str = "stado-apple-challenge-preparation";

pub fn run(context: &specs::Context) -> Result<(), String> {
    console::require_product_dispatch(context)?;
    let host = declared_host(context)?;

    let readiness_before = prompt_free_readiness(context)?;
    if readiness_before
        .get("accessibility")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err("the existing CuaDriver daemon must report Accessibility ready without prompting before app launch".into());
    }

    let driver = crate::specs::cua::common::driver(context)?;
    let mut app = None;
    let mut failure = None;
    let result = (|| {
        let launched = console::launch_console(context, &driver)?;
        app = Some(launched.clone());
        select_host(&driver, &launched, &host)?;
        confirm_command(context, &driver, &launched, &host)?;
        read_readiness(context, &driver, &launched, &host)?;
        prepare_capture(context, &driver, &launched, &host)
    })();
    if let Err(error) = result {
        failure = Some(error);
    }

    // The product operation must not have moved CuaDriver's own
    // permissions; this journey only observes them.
    match console::read_prompt_free_cua_readiness(&driver) {
        Ok(after) if after == readiness_before => {}
        Ok(_) => {
            failure = Some(add_failure(
                failure,
                "CuaDriver readiness changed while the Apple-only product operation ran".into(),
                "checking unchanged CuaDriver readiness",
            ));
        }
        Err(error) => {
            failure = Some(add_failure(
                failure,
                error,
                "checking unchanged CuaDriver readiness",
            ));
        }
    }
    if let Some(app) = &app {
        if let Err(error) = console::dump_windows(context, &driver, app.pid, SLUG) {
            failure = Some(add_failure(
                failure,
                error,
                "writing the final native accessibility tree",
            ));
        }
        driver.quit_app(app.pid);
    }
    failure.map_or(Ok(()), Err)
}

/// The selected host's screen must show the exact command the product
/// will run — character for character, not a paraphrase.
fn confirm_command(
    context: &specs::Context,
    driver: &crate::cua::Driver,
    app: &crate::cua::App,
    host: &str,
) -> Result<(), String> {
    let expected_command = preparation_command(host);
    console::wait_for_screen(
        driver,
        app.pid,
        app.window_id,
        |tree| tree.contains("Apple code capture") && tree.contains(&expected_command),
        "a state this journey reads",
        GATES,
    )?;
    let selected = console::capture(
        context,
        driver,
        app.pid,
        app.window_id,
        SLUG,
        "selected-host",
    )?;
    let command =
        console::assert_field(&selected, "Command", None::<fn(&str) -> bool>, "the field")?;
    if command != expected_command {
        return Err(format!(
            "Command reads {command:?}, expected {expected_command:?}"
        ));
    }
    Ok(())
}

/// Read-only step: ask the product for Apple readiness on this host and
/// check it answered about this host.
fn read_readiness(
    context: &specs::Context,
    driver: &crate::cua::Driver,
    app: &crate::cua::App,
    host: &str,
) -> Result<(), String> {
    console::click(driver, app.pid, app.window_id, "Read Apple readiness")?;
    let dismiss = Regex::new(r"AX\w*Button \(Dismiss\)").unwrap();
    let read_marker = format!("Apple code capture status read on {host}");
    console::wait_for_screen(
        driver,
        app.pid,
        app.window_id,
        |tree| tree.contains(&read_marker) || dismiss.is_match(tree),
        "a state this journey reads",
        PREPARATION,
    )?;
    let readiness = console::capture(
        context,
        driver,
        app.pid,
        app.window_id,
        SLUG,
        "readiness-report",
    )?;
    if !readiness.tree.contains(&read_marker) {
        return Err(format!(
            "Stado Desktop refused the read-only readiness request:\n{}",
            exact_refusal(&readiness)
        ));
    }
    reported_host(&readiness, host)
}

/// The real preparation: install or reuse the signed helper, then check
/// the product reported the helper version, the Accessibility grant,
/// the prompt-free exercise, and which of install or reuse happened.
fn prepare_capture(
    context: &specs::Context,
    driver: &crate::cua::Driver,
    app: &crate::cua::App,
    host: &str,
) -> Result<(), String> {
    console::click(driver, app.pid, app.window_id, "Prepare Apple code capture")?;
    let dismiss = Regex::new(r"AX\w*Button \(Dismiss\)").unwrap();
    let ready_marker = format!("Apple code capture is ready on {host}");
    let read_marker = format!("Apple code capture status read on {host}");
    console::wait_for_screen(
        driver,
        app.pid,
        app.window_id,
        |tree| {
            tree.contains(&ready_marker)
                || tree.contains("Apple code capture is unavailable")
                || (dismiss.is_match(tree) && !tree.contains(&read_marker))
        },
        "a state this journey reads",
        PREPARATION,
    )?;
    let observed = console::capture(
        context,
        driver,
        app.pid,
        app.window_id,
        SLUG,
        "preparation-report",
    )?;
    if !observed.tree.contains(&ready_marker) {
        return Err(format!(
            "Stado Desktop refused Apple challenge preparation for {host:?}; exact visible refusal:\n{}",
            exact_refusal(&observed)
        ));
    }
    reported_host(&observed, host)?;

    let destination = console::assert_field(
        &observed,
        "Host-control destination",
        None::<fn(&str) -> bool>,
        "the field",
    )?;
    if destination.is_empty() {
        return Err("the preparation report omitted its real host-control destination".into());
    }
    if !report_item(
        &observed.tree,
        "apple-challenge-helper-version",
        APPLE_HELPER_VERSION,
    ) {
        return Err(format!(
            "the product did not report Apple helper version {APPLE_HELPER_VERSION}"
        ));
    }
    if !report_item(&observed.tree, "apple-challenge-accessibility", "granted") {
        return Err("the product did not read back the Apple helper Accessibility grant".into());
    }
    if !report_item(&observed.tree, "apple-challenge-ready", "yes") {
        return Err("the product did not exercise the signed helper prompt-free in the registry-bound Aqua session".into());
    }
    if !Regex::new(r#"(?i)apple-challenge-helper:\s*(?:installed|reused)(?:[\s"),]|$)"#)
        .unwrap()
        .is_match(&observed.tree)
    {
        return Err(
            "the product did not report whether the real signed helper was installed or reused"
                .into(),
        );
    }
    Ok(())
}

/// The report must name the host the operator asked about.
fn reported_host(view: &console::View, host: &str) -> Result<(), String> {
    let reported =
        console::assert_field(view, "Reported host", None::<fn(&str) -> bool>, "the field")?;
    if reported != host {
        return Err(format!(
            "Reported host reads {reported:?}, expected {host:?}"
        ));
    }
    Ok(())
}
