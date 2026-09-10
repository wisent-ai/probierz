use serde_json::json;
use super::*;
pub(crate) fn observe(
    driver: &Driver,
    pid: u32,
    window_id: u64,
    label: &str,
    trace: &mut Vec<Value>,
    last_tree: &mut Option<String>,
    screenshot: Option<&Path>,
    force: bool,
) -> Result<View, String> {
    let snapshot = driver.snapshot_to(pid, window_id, screenshot)?;
    let view = view(snapshot);
    if force || last_tree.as_deref() != Some(&view.tree) {
        trace.push(json!({
            "observedAt": observed_at(),
            "label": label,
            "snapshotID": view.snapshot_id,
            "tree": view.tree,
            "elements": view.elements,
        }));
        *last_tree = Some(view.tree.clone());
    }
    Ok(view)
}

pub(crate) fn capture(
    context: &specs::Context,
    driver: &Driver,
    pid: u32,
    window_id: u64,
    name: &str,
    trace: &mut Vec<Value>,
    last_tree: &mut Option<String>,
) -> Result<View, String> {
    let file = context
        .artifacts
        .join(format!("{}-{name}.png", context.title));
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let view = observe(
        driver,
        pid,
        window_id,
        &format!("capture:{name}"),
        trace,
        last_tree,
        Some(&file),
        true,
    )?;
    let metadata = fs::metadata(&file)
        .map_err(|_| format!("cua-driver wrote no screenshot at {}", file.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!(
            "cua-driver wrote no screenshot at {}",
            file.display()
        ));
    }
    context.media("screenshot", file);
    Ok(view)
}

pub(crate) fn authorized(tree: &str) -> Result<(), String> {
    let visible = tree.contains("id=wisent.auth.screen")
        || tree.contains("Sign in with your Wisent account")
        || tree.contains("AXButton (Continue with GitHub)");
    if visible {
        Err("The Stado GUI worker has no authorized Wisent identity for Jeden Desktop; this journey refuses to request credentials or trigger consent UI".to_string())
    } else {
        Ok(())
    }
}

pub(crate) fn wait_for_shell(
    driver: &Driver,
    app: &App,
    trace: &mut Vec<Value>,
    last_tree: &mut Option<String>,
) -> Result<View, String> {
    let deadline = Instant::now() + SHELL_TIMEOUT;
    let mut last = None;
    while Instant::now() < deadline {
        let current = observe(
            driver,
            app.pid,
            app.window_id,
            "wait:authorized-shell",
            trace,
            last_tree,
            None,
            false,
        )?;
        authorized(&current.tree)?;
        if current.tree.contains("AXButton (Settings)") {
            return Ok(current);
        }
        last = Some(current);
        thread::sleep(POLL);
    }
    Err(format!(
        "Jeden Desktop did not expose its authorized Settings navigation within 45000 ms; last accessibility tree: {}",
        common::tail(&last.map(|view| view.tree).unwrap_or_default(), 1500)
    ))
}

pub(crate) fn click_fresh(
    driver: &Driver,
    app: &App,
    needle: &str,
    label: &str,
    trace: &mut Vec<Value>,
    last_tree: &mut Option<String>,
) -> Result<(), String> {
    let before = observe(
        driver,
        app.pid,
        app.window_id,
        &format!("before-action:{label}"),
        trace,
        last_tree,
        None,
        true,
    )?;
    authorized(&before.tree)?;
    let index = cua::element_index_of(&before.tree, needle)?;
    let element = before
        .elements
        .iter()
        .find(|element| element.get("element_index").and_then(Value::as_u64) == Some(index))
        .ok_or_else(|| format!("no indexed element matching {needle:?} in tree"))?;
    let result = driver.click_element(app.pid, app.window_id, &before.snapshot, element)?;
    if result.get("status").and_then(Value::as_str) == Some("refused") {
        return Err(format!("cua-driver refused the {label} action: {result}"));
    }
    Ok(())
}

