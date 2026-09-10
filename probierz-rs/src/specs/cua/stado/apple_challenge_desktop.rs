use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use regex::Regex;
use serde_json::Value;

use crate::specs;

use super::console as console;

const GATES: Duration = Duration::from_secs(180);
const PREPARATION: Duration = Duration::from_secs(360);
const APPLE_HELPER_VERSION: &str = "2";

fn prompt_free_readiness(context: &specs::Context) -> Result<Value, String> {
    let binary = context
        .optional("CUA_DRIVER_BIN")
        .or_else(|| std::env::var("CUA_DRIVER_BIN").ok())
        .unwrap_or_else(|| {
            let bundled = Path::new("/Applications/CuaDriver.app/Contents/MacOS/cua-driver");
            if cfg!(target_os = "macos") && bundled.is_file() {
                bundled.to_string_lossy().into_owned()
            } else {
                "cua-driver".to_string()
            }
        });
    let socket = context
        .optional("CUA_DRIVER_SOCKET")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CUA_DRIVER_SOCKET").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join("Library/Caches/cua-driver/probierz.sock"))
        })
        .ok_or_else(|| "HOME is required to locate the Probierz CuaDriver socket".to_string())?;
    let output = Command::new(binary)
        .arg("call")
        .arg("check_permissions")
        .arg(r#"{"prompt":false}"#)
        .arg("--socket")
        .arg(socket)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let response: Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    Ok(response.get("permissions").cloned().unwrap_or(response))
}

fn quoted(argument: &str) -> String {
    if Regex::new(r"^[A-Za-z0-9_\-./:=@+,]+$")
        .unwrap()
        .is_match(argument)
    {
        argument.to_string()
    } else {
        format!(
            "\"{}\"",
            argument.replace('\\', "\\\\").replace('"', "\\\"")
        )
    }
}

fn preparation_command(host: &str) -> String {
    format!(
        "stado host gui-automation grant-accessibility {} --apple-only --json",
        quoted(host)
    )
}

fn exact_refusal(view: &console::View) -> String {
    let lines = view.tree.lines().collect::<Vec<_>>();
    let marker = lines.iter().position(|line| {
        line.contains("Apple code capture is unavailable")
            || Regex::new(r"AX\w*Button \(Dismiss\)")
                .unwrap()
                .is_match(line)
    });
    let (start, end) = marker.map_or_else(
        || (lines.len().saturating_sub(30), lines.len()),
        |position| {
            (
                position.saturating_sub(16),
                lines.len().min(position.saturating_add(8)),
            )
        },
    );
    lines[start..end]
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn add_failure(current: Option<String>, next: String, context: &str) -> String {
    match current {
        Some(current) => format!("{current}; additionally {context}: {next}"),
        None => next,
    }
}

fn report_item(tree: &str, name: &str, value: &str) -> bool {
    Regex::new(&format!(
        r#"(?i){}:\s*{}(?:[\s"),]|$)"#,
        regex::escape(name),
        regex::escape(value)
    ))
    .unwrap()
    .is_match(tree)
}

pub fn run(context: &specs::Context) -> Result<(), String> {
    console::require_product_dispatch(context)?;
    let raw_host = std::env::var("STADO_APPLE_PREPARATION_HOST")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| context.optional("STADO_APPLE_PREPARATION_HOST"))
        .ok_or_else(|| {
            "STADO_APPLE_PREPARATION_HOST must explicitly name the dedicated Stado host".to_string()
        })?;
    let host = raw_host.trim();
    if host != raw_host {
        return Err("STADO_APPLE_PREPARATION_HOST must not contain surrounding whitespace".into());
    }
    if host.is_empty() {
        return Err("STADO_APPLE_PREPARATION_HOST must not be empty".into());
    }
    if host
        .chars()
        .any(|character| matches!(character, '\r' | '\n' | '\0'))
    {
        return Err("STADO_APPLE_PREPARATION_HOST contains a control character".into());
    }

    let readiness_before = prompt_free_readiness(context)?;
    if readiness_before
        .get("accessibility")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err("the existing CuaDriver daemon must report Accessibility ready without prompting before app launch".into());
    }

    let expected_command = preparation_command(host);
    let driver = crate::specs::cua::common::driver(context)?;
    let mut app = None;
    let mut failure = None;
    let result = (|| {
        let launched = console::launch_console(context, &driver)?;
        app = Some(launched.clone());
        console::open_screen(
            &driver,
            launched.pid,
            launched.window_id,
            "Hosts",
            |tree| {
                Regex::new(r"AX\w*Button \(All hosts")
                    .unwrap()
                    .is_match(tree)
            },
            "/AX\\w*Button \\(All hosts/",
            &["No host inventory", "No registered hosts"],
            "Refresh",
            GATES,
        )?;
        let row_pattern =
            Regex::new(&format!(r"AX\w*Button \({}(?:,|\))", regex::escape(host))).unwrap();
        let hosts = console::wait_for_screen(
            &driver,
            launched.pid,
            launched.window_id,
            |tree| row_pattern.is_match(tree),
            &format!("/{}/", row_pattern.as_str()),
            GATES,
        )?;
        let matching = console::row_buttons(&hosts, 3)
            .into_iter()
            .filter(|row| row.label == host || row.label.starts_with(&format!("{host},")))
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(format!(
                "the real Hosts table must contain exactly one row for {host:?}; rows: {}",
                {
                    let rows = console::row_buttons(&hosts, 3)
                        .into_iter()
                        .map(|row| row.label)
                        .collect::<Vec<_>>()
                        .join(" | ");
                    if rows.is_empty() {
                        "none".to_string()
                    } else {
                        rows
                    }
                }
            ));
        }
        console::click(
            &driver,
            launched.pid,
            launched.window_id,
            &matching[0].label,
        )?;
        console::wait_for_screen(
            &driver,
            launched.pid,
            launched.window_id,
            |tree| tree.contains("Apple code capture") && tree.contains(&expected_command),
            "a state this journey reads",
            GATES,
        )?;
        let selected = console::capture(
            context,
            &driver,
            launched.pid,
            launched.window_id,
            "stado-apple-challenge-preparation",
            "selected-host",
        )?;
        let command =
            console::assert_field(&selected, "Command", None::<fn(&str) -> bool>, "the field")?;
        if command != expected_command {
            return Err(format!(
                "Command reads {command:?}, expected {expected_command:?}"
            ));
        }
        console::click(
            &driver,
            launched.pid,
            launched.window_id,
            "Read Apple readiness",
        )?;
        let readiness_pattern = Regex::new(r"AX\w*Button \(Dismiss\)").unwrap();
        console::wait_for_screen(
            &driver,
            launched.pid,
            launched.window_id,
            |tree| {
                tree.contains(&format!("Apple code capture status read on {host}"))
                    || readiness_pattern.is_match(tree)
            },
            "a state this journey reads",
            PREPARATION,
        )?;
        let readiness = console::capture(
            context,
            &driver,
            launched.pid,
            launched.window_id,
            "stado-apple-challenge-preparation",
            "readiness-report",
        )?;
        if !readiness
            .tree
            .contains(&format!("Apple code capture status read on {host}"))
        {
            return Err(format!(
                "Stado Desktop refused the read-only readiness request:\n{}",
                exact_refusal(&readiness)
            ));
        }
        let reported = console::assert_field(
            &readiness,
            "Reported host",
            None::<fn(&str) -> bool>,
            "the field",
        )?;
        if reported != host {
            return Err(format!(
                "Reported host reads {reported:?}, expected {host:?}"
            ));
        }

        console::click(
            &driver,
            launched.pid,
            launched.window_id,
            "Prepare Apple code capture",
        )?;
        let dismiss = Regex::new(r"AX\w*Button \(Dismiss\)").unwrap();
        console::wait_for_screen(
            &driver,
            launched.pid,
            launched.window_id,
            |tree| {
                tree.contains(&format!("Apple code capture is ready on {host}"))
                    || tree.contains("Apple code capture is unavailable")
                    || (dismiss.is_match(tree)
                        && !tree.contains(&format!("Apple code capture status read on {host}")))
            },
            "a state this journey reads",
            PREPARATION,
        )?;
        let observed = console::capture(
            context,
            &driver,
            launched.pid,
            launched.window_id,
            "stado-apple-challenge-preparation",
            "preparation-report",
        )?;
        if !observed
            .tree
            .contains(&format!("Apple code capture is ready on {host}"))
        {
            return Err(format!(
                "Stado Desktop refused Apple challenge preparation for {host:?}; exact visible refusal:\n{}",
                exact_refusal(&observed)
            ));
        }
        let reported = console::assert_field(
            &observed,
            "Reported host",
            None::<fn(&str) -> bool>,
            "the field",
        )?;
        if reported != host {
            return Err(format!(
                "Reported host reads {reported:?}, expected {host:?}"
            ));
        }
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
            return Err(
                "the product did not read back the Apple helper Accessibility grant".into(),
            );
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
    })();
    if let Err(error) = result {
        failure = Some(error);
    }

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
        if let Err(error) = console::dump_windows(
            context,
            &driver,
            app.pid,
            "stado-apple-challenge-preparation",
        ) {
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
