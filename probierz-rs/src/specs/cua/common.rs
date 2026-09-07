use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::Value;

use crate::cua::{self, Driver, Snapshot};
use crate::specs;
pub fn driver(context: &specs::Context) -> Result<Driver, String> {
    Driver::connect_config(
        context.optional("CUA_DRIVER_BIN"),
        context.optional("CUA_DRIVER_SOCKET").map(PathBuf::from),
    )
}

pub fn executable(context: &specs::Context, what: &str) -> Result<PathBuf, String> {
    let value = context.required("CUA_APP_EXECUTABLE", what)?;
    let path = PathBuf::from(&value);
    if !path.is_absolute() {
        return Err(format!(
            "CUA_APP_EXECUTABLE must be an absolute path, received {value:?}"
        ));
    }
    if !path.is_file() {
        return Err(format!(
            "CUA_APP_EXECUTABLE is not a file: {}",
            path.display()
        ));
    }
    Ok(path)
}

pub fn optional_path(context: &specs::Context, name: &str, fallback: &str) -> PathBuf {
    PathBuf::from(
        context
            .optional(name)
            .unwrap_or_else(|| fallback.to_string()),
    )
}

pub fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}{:x}", std::process::id(), nanos)
}

pub fn elements(snapshot: &Snapshot) -> &[Value] {
    &snapshot.elements
}

pub fn is_button(element: &Value) -> bool {
    cua::element_role(element).contains("Button")
}

pub fn capture(
    context: &specs::Context,
    driver: &Driver,
    pid: u32,
    window_id: u64,
    name: &str,
) -> Result<Snapshot, String> {
    let file = context
        .artifacts
        .join(format!("{}-{name}.png", context.title));
    let snapshot = driver.screenshot(pid, window_id, &file)?;
    context.media("screenshot", file);
    Ok(snapshot)
}

pub fn dump_tree(context: &specs::Context, name: &str, tree: &str) -> Result<PathBuf, String> {
    let file = context
        .artifacts
        .join(format!("{}-{name}.tree.txt", context.title));
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(&file, tree).map_err(|error| format!("{}: {error}", file.display()))?;
    Ok(file)
}

pub fn wait_for_window_text(
    context: &specs::Context,
    driver: &Driver,
    pid: u32,
    needle: &str,
    timeout: Duration,
) -> Result<(u64, Snapshot), String> {
    let deadline = Instant::now() + timeout;
    let mut widest = String::new();
    while Instant::now() < deadline {
        for window in driver.list_windows(Some(pid))? {
            if window
                .get("layer")
                .and_then(Value::as_i64)
                .is_some_and(|layer| layer != 0)
            {
                continue;
            }
            let Some(window_id) = window.get("window_id").and_then(Value::as_u64) else {
                continue;
            };
            let snapshot = driver.snapshot(pid, window_id)?;
            if snapshot.tree.contains(needle) {
                return Ok((window_id, snapshot));
            }
            if snapshot.tree.len() > widest.len() {
                widest = snapshot.tree;
            }
        }
        thread::sleep(Duration::from_millis(300));
    }
    let slug: String = needle
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .take(60)
        .collect();
    let file = dump_tree(context, &format!("timeout-{slug}"), &widest)?;
    Err(format!(
        "timed out waiting for {needle:?}; widest tree written to {}; tail: {}",
        file.display(),
        tail(&widest, 1200)
    ))
}

pub fn activate<M, S>(
    context: &specs::Context,
    driver: &Driver,
    pid: u32,
    window_id: u64,
    label: &str,
    matches: M,
    settled: S,
    timeout: Duration,
) -> Result<String, String>
where
    M: Fn(&Value) -> bool,
    S: Fn(&str) -> bool,
{
    let mut last_error = None;
    for rung in 0..3 {
        let snapshot = driver.snapshot(pid, window_id)?;
        let Some(element) = elements(&snapshot).iter().find(|element| matches(element)) else {
            let slug: String = label
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            let _ = dump_tree(context, &format!("activate-{slug}-missing"), &snapshot.tree);
            return Err(format!("no element to press for {label}"));
        };
        let pressed = match rung {
            0 => driver.click_element(pid, window_id, &snapshot, element),
            1 | 2 => {
                let frame = cua::element_frame(element)
                    .ok_or_else(|| format!("element for {label} carries no frame to click"))?;
                driver.click_pixel(
                    pid,
                    window_id,
                    frame.x + frame.width / 2.0,
                    frame.y + frame.height / 2.0,
                    rung == 2,
                )
            }
            _ => unreachable!(),
        };
        if let Err(error) = pressed {
            last_error = Some(error);
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            thread::sleep(Duration::from_millis(400));
            let tree = driver.snapshot(pid, window_id)?.tree;
            if settled(&tree) {
                return Ok(tree);
            }
        }
        if let Ok(after) = driver.snapshot(pid, window_id) {
            let slug: String = label
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            let _ = dump_tree(
                context,
                &format!("activate-{slug}-rung-{}", rung + 1),
                &after.tree,
            );
        }
    }
    Err(format!(
        "pressing {label} never settled{}",
        last_error
            .map(|error| format!(" (last press error: {error})"))
            .unwrap_or_default()
    ))
}

pub fn type_field(
    context: &specs::Context,
    driver: &Driver,
    pid: u32,
    window_id: u64,
    label: &str,
    value: &str,
    allow_foreground_retry: bool,
) -> Result<(), String> {
    let modes: &[bool] = if allow_foreground_retry {
        &[false, true]
    } else {
        &[false]
    };
    let mut last = String::new();
    for foreground in modes {
        let snapshot = driver.snapshot(pid, window_id)?;
        let element = elements(&snapshot).iter().find(|candidate| {
            cua::element_label(candidate) == label
                && (cua::element_role(candidate).contains("TextField")
                    || cua::element_role(candidate).contains("SecureTextField"))
        });
        let Some(element) = element else {
            let slug: String = label
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            let _ = dump_tree(context, &format!("field-{slug}-missing"), &snapshot.tree);
            return Err(format!("no editable field labelled {label}"));
        };
        driver.type_text(pid, window_id, &snapshot, element, value, *foreground)?;
        thread::sleep(Duration::from_millis(600));
        let after = driver.snapshot(pid, window_id)?;
        let after_element = elements(&after)
            .iter()
            .find(|candidate| cua::element_label(candidate) == label)
            .ok_or_else(|| format!("{label} must remain addressable after typing"))?;
        last = cua::element_value(after_element).to_string();
        if cua::element_role(after_element).contains("Secure") || last.contains(value) {
            return Ok(());
        }
    }
    Err(format!(
        "typing {value:?} into {label} did not land; the field holds {last:?}"
    ))
}

pub fn require_contains(
    tree: &str,
    needle: &str,
    failure: impl FnOnce() -> String,
) -> Result<(), String> {
    if tree.contains(needle) {
        Ok(())
    } else {
        Err(failure())
    }
}

pub fn require_absent(
    tree: &str,
    needle: &str,
    failure: impl FnOnce() -> String,
) -> Result<(), String> {
    if !tree.contains(needle) {
        Ok(())
    } else {
        Err(failure())
    }
}

pub fn require_regex(
    tree: &str,
    pattern: &str,
    failure: impl FnOnce() -> String,
) -> Result<(), String> {
    let regex = Regex::new(pattern)
        .map_err(|error| format!("invalid journey regular expression {pattern:?}: {error}"))?;
    if regex.is_match(tree) {
        Ok(())
    } else {
        Err(failure())
    }
}

pub fn static_texts(tree: &str) -> Vec<String> {
    let regex = Regex::new(r#"AXStaticText = "((?:\\.|[^"\\])*)""#).expect("static text regex");
    regex
        .captures_iter(tree)
        .filter_map(|capture| {
            let quoted = format!("\"{}\"", &capture[1]);
            serde_json::from_str::<String>(&quoted)
                .ok()
                .or_else(|| Some(capture[1].to_string()))
        })
        .collect()
}

pub fn tail(text: &str, count: usize) -> String {
    let mut chars: Vec<char> = text.chars().rev().take(count).collect();
    chars.reverse();
    chars.into_iter().collect()
}
