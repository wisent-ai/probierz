use std::collections::HashMap;
use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::Value;

use crate::cua::{self, App, Driver, Snapshot};
use crate::specs;

use super::common;

const POLL: Duration = Duration::from_millis(500);
const CONSOLE_CHROME: &[&str] = &[
    "close",
    "minimize",
    "minimise",
    "zoom",
    "fullscreen",
    "Posture",
    "Queue",
    "Hosts",
    "Services",
    "Disk",
    "Registry",
    "Releases",
    "Deployments",
    "Refresh",
    "Re-diagnose",
    "Retry",
    "Dismiss",
    "Show them",
    "Read again",
    "Clear filters",
    "All hosts",
    "Not claiming",
    "Unavailable",
    "Stale",
    "Live",
    "Declared",
    "Undeclared",
    "Pinned only",
    "All units",
    "Serving replaced code",
    "Unowned processes",
];

#[derive(Clone, Debug)]
pub struct View {
    pub tree: String,
    pub elements: Vec<Value>,
    snapshot: Snapshot,
}

#[derive(Clone, Debug)]
pub struct Button {
    pub index: u64,
    pub label: String,
}

pub fn read_prompt_free_cua_readiness(driver: &Driver) -> Result<Value, String> {
    let response = driver.call("check_permissions", serde_json::json!({ "prompt": false }))?;
    Ok(response.get("permissions").cloned().unwrap_or(response))
}
pub fn require_product_dispatch(context: &specs::Context) -> Result<std::path::PathBuf, String> {
    let source = context
        .optional("PROBIERZ_APP_SOURCE")
        .ok_or_else(|| "PROBIERZ_APP_SOURCE must identify the staged Stado source".to_string())?;
    let journeys = context.optional("PROBIERZ_JOURNEYS").ok_or_else(|| {
        "PROBIERZ_JOURNEYS must name the journeys selected by Probierz".to_string()
    })?;
    let journeys = journeys
        .split(',')
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if journeys.is_empty() {
        return Err("PROBIERZ_JOURNEYS must name the journeys selected by Probierz".into());
    }
    for journey in journeys {
        if !matches!(
            journey,
            "host-dynamic-capacity" | "service-convergence" | "apple-challenge-desktop"
        ) {
            return Err(format!(
                "Unmapped Stado desktop journey selected by Probierz: {journey}"
            ));
        }
    }
    Ok(source.into())
}

pub fn read_window(driver: &Driver, pid: u32, window_id: u64) -> Result<View, String> {
    Ok(view_from(driver.snapshot(pid, window_id)?))
}

fn view_from(snapshot: Snapshot) -> View {
    View {
        tree: snapshot.tree.clone(),
        elements: snapshot.elements.clone(),
        snapshot,
    }
}

pub fn capture(
    context: &specs::Context,
    driver: &Driver,
    pid: u32,
    window_id: u64,
    slug: &str,
    name: &str,
) -> Result<View, String> {
    let file = context.artifacts.join(format!("{slug}-{name}.png"));
    let snapshot = driver.screenshot(pid, window_id, &file)?;
    let metadata = fs::metadata(&file).map_err(|error| format!("{}: {error}", file.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "cua-driver wrote no screenshot at {}",
            file.display()
        ));
    }
    context.media("screenshot", file);
    Ok(view_from(snapshot))
}

pub fn dump_windows(
    context: &specs::Context,
    driver: &Driver,
    pid: u32,
    slug: &str,
) -> Result<(), String> {
    let file = context.artifacts.join(format!("{slug}-ax-tree.txt"));
    let mut sections = Vec::new();
    for window in driver.list_windows(Some(pid))? {
        let Some(window_id) = window.get("window_id").and_then(Value::as_u64) else {
            continue;
        };
        let body = match read_window(driver, pid, window_id) {
            Ok(view) => format!(
                "{}\n\n## elements\n{}",
                view.tree,
                serde_json::to_string_pretty(&view.elements).unwrap_or_default()
            ),
            Err(error) => format!("unreadable: {error}"),
        };
        sections.push(format!(
            "# window {window_id} {} {}\n{body}",
            window.get("title").unwrap_or(&Value::String(String::new())),
            window
                .get("bounds")
                .unwrap_or(&Value::Object(Default::default())),
        ));
    }
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(&file, format!("{}\n", sections.join("\n\n")))
        .map_err(|error| format!("{}: {error}", file.display()))
}

fn element_of(view: &View, index: u64) -> Option<&Value> {
    view.elements
        .iter()
        .find(|element| element.get("element_index").and_then(Value::as_u64) == Some(index))
}

fn refusal(result: &Value) -> Option<&str> {
    (result.get("status").and_then(Value::as_str) == Some("refused")).then(|| {
        result
            .pointer("/refusal/code")
            .and_then(Value::as_str)
            .unwrap_or("refused")
    })
}

pub fn click(driver: &Driver, pid: u32, window_id: u64, label: &str) -> Result<Value, String> {
    let mut last = Value::Null;
    for _ in 0..5 {
        let view = read_window(driver, pid, window_id)?;
        let target = button(&view, label)?;
        let element = element_of(&view, target.index)
            .ok_or_else(|| format!("no indexed element matching {:?} in tree", target.label))?;
        let result = driver.click_element(pid, window_id, &view.snapshot, element)?;
        let stale = refusal(&result) == Some("stale_element_token");
        last = result;
        if !stale {
            return Ok(last);
        }
    }
    Ok(last)
}

pub fn activate<F>(
    driver: &Driver,
    pid: u32,
    window_id: u64,
    label: &str,
    description: &str,
    matches: F,
    timeout: Duration,
) -> Result<(u64, View, Value), String>
where
    F: Fn(&str) -> bool,
{
    let pressed = click(driver, pid, window_id, label)?;
    if let Some((opened_window, opened)) = wait_for_any_window(driver, pid, &matches, timeout)? {
        return Ok((opened_window, opened, pressed));
    }
    Err(format!(
        "pressing {label:?} showed nothing carrying {description} within {} ms; the driver answered {}",
        timeout.as_millis(),
        common::tail(&pressed.to_string(), 300)
    ))
}

pub fn buttons(view: &View) -> Vec<Button> {
    let label = Regex::new(r"AX\w*Button \(([^)]*)\)").expect("button regex");
    let index = Regex::new(r"\[(\d+)\]").expect("index regex");
    view.tree
        .lines()
        .filter_map(|line| {
            let label = label.captures(line)?.get(1)?.as_str().to_string();
            let index = index.captures(line)?.get(1)?.as_str().parse().ok()?;
            Some(Button { index, label })
        })
        .collect()
}

pub fn find_button(view: &View, label: &str) -> Option<Button> {
    buttons(view)
        .into_iter()
        .find(|item| item.label == label || item.label.starts_with(&format!("{label},")))
}

pub fn button(view: &View, label: &str) -> Result<Button, String> {
    find_button(view, label).ok_or_else(|| {
        let labels: Vec<String> = buttons(view).into_iter().map(|item| item.label).collect();
        format!(
            "no button labelled {label:?}; buttons on screen: {}",
            if labels.is_empty() {
                "none".to_string()
            } else {
                labels.join(" | ")
            }
        )
    })
}

pub fn assert_refused_control(view: &View, label: &str) -> Result<String, String> {
    let pattern = Regex::new(&format!(
        r"^\s*-\s+AX\w*Button \({}\)\s*$",
        regex::escape(label)
    ))
    .map_err(|error| error.to_string())?;
    let line = view
        .tree
        .lines()
        .find(|line| pattern.is_match(line))
        .ok_or_else(|| {
            format!(
                "the screen renders no {label:?} control at all; buttons on screen: {}",
                buttons(view)
                    .into_iter()
                    .map(|item| item.label)
                    .collect::<Vec<_>>()
                    .join(" | ")
            )
        })?;
    if let Some(actionable) = find_button(view, label) {
        return Err(format!(
            "{label:?} is offered as an actionable control: {actionable:?}"
        ));
    }
    Ok(line.trim().to_string())
}

pub fn row_buttons(view: &View, minimum_fields: usize) -> Vec<Button> {
    let by_index: HashMap<u64, &Value> = view
        .elements
        .iter()
        .filter_map(|element| Some((element.get("element_index")?.as_u64()?, element)))
        .collect();
    let container = Regex::new(r"(?i)ProviderGroup|ScrollArea|Table|Outline|List|Grid")
        .expect("container regex");
    buttons(view)
        .into_iter()
        .filter(|item| {
            if CONSOLE_CHROME
                .iter()
                .any(|known| item.label == *known || item.label.starts_with(&format!("{known},")))
            {
                return false;
            }
            let parent = by_index
                .get(&item.index)
                .and_then(|element| element.get("parent_index"))
                .and_then(Value::as_u64)
                .and_then(|index| by_index.get(&index));
            if !parent.is_some_and(|parent| container.is_match(cua::element_role(parent))) {
                return false;
            }
            item.label.matches(',').count() >= minimum_fields.saturating_sub(1)
        })
        .collect()
}

pub fn poll<F>(
    driver: &Driver,
    pid: u32,
    window_id: u64,
    matches: F,
    timeout: Duration,
) -> Result<Option<View>, String>
where
    F: Fn(&str) -> bool,
{
    let deadline = Instant::now() + timeout;
    let mut view = read_window(driver, pid, window_id)?;
    while !matches(&view.tree) {
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(POLL);
        view = read_window(driver, pid, window_id)?;
    }
    Ok(Some(view))
}

pub fn wait_for_screen<F>(
    driver: &Driver,
    pid: u32,
    window_id: u64,
    matches: F,
    description: &str,
    timeout: Duration,
) -> Result<View, String>
where
    F: Fn(&str) -> bool,
{
    if let Some(view) = poll(driver, pid, window_id, matches, timeout)? {
        return Ok(view);
    }
    let last = read_window(driver, pid, window_id)?;
    Err(format!(
        "timed out after {} ms waiting for {description}; last tree: {}",
        timeout.as_millis(),
        common::tail(&last.tree, 2500)
    ))
}

pub fn windows_of(driver: &Driver, pid: u32) -> Result<Vec<Value>, String> {
    driver.list_windows(Some(pid))
}

pub fn wait_for_any_window<F>(
    driver: &Driver,
    pid: u32,
    matches: &F,
    timeout: Duration,
) -> Result<Option<(u64, View)>, String>
where
    F: Fn(&str) -> bool,
{
    let deadline = Instant::now() + timeout;
    loop {
        for window in windows_of(driver, pid)? {
            let Some(window_id) = window.get("window_id").and_then(Value::as_u64) else {
                continue;
            };
            let Ok(view) = read_window(driver, pid, window_id) else {
                continue;
            };
            if matches(&view.tree) {
                return Ok(Some((window_id, view)));
            }
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(POLL);
    }
}

pub fn field_values(view: &View, label: &str) -> Vec<String> {
    let upper = label.to_uppercase();
    let combined: Vec<String> = view
        .elements
        .iter()
        .filter_map(|element| {
            let text = cua::element_label(element);
            (text == upper || text.starts_with(&format!("{upper},")))
                .then(|| clean(&text[upper.len()..]))
        })
        .collect();
    if combined.iter().any(|value| !value.is_empty()) {
        return combined;
    }
    let lines: Vec<&str> = view.tree.lines().collect();
    let following = Regex::new(r#"= "([^"]*)"|\(([^)]*)\)"#).expect("following value regex");
    let mut values = Vec::new();
    for (position, line) in lines.iter().enumerate() {
        if !line.contains(&format!("\"{upper}\"")) && !line.contains(&format!("({upper})")) {
            continue;
        }
        if let Some(capture) = lines
            .get(position + 1)
            .and_then(|line| following.captures(line))
        {
            let value = clean(
                capture
                    .get(1)
                    .or_else(|| capture.get(2))
                    .map(|m| m.as_str())
                    .unwrap_or_default(),
            );
            if !value.is_empty() {
                values.push(value);
            }
        }
    }
    if values.is_empty() {
        combined
    } else {
        values
    }
}

fn clean(text: &str) -> String {
    text.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '"' | ',' | ':' | ';' | '=')
    })
    .trim()
    .to_string()
}

pub fn assert_field<F>(
    view: &View,
    label: &str,
    pattern: Option<F>,
    description: &str,
) -> Result<String, String>
where
    F: Fn(&str) -> bool,
{
    let values = field_values(view, label);
    if values.is_empty() {
        return Err(format!(
            "the inspector renders no {label} field; tree: {}",
            common::tail(&view.tree, 2000)
        ));
    }
    let value = match pattern {
        Some(pattern) => values.iter().find(|value| pattern(value)),
        None => values.first(),
    };
    value
        .cloned()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{label} reads {values:?}, which does not answer {description}"))
}

pub fn launch_console(context: &specs::Context, driver: &Driver) -> Result<App, String> {
    launch_console_with(context, driver, &Default::default(), &[])
}

pub fn launch_console_with(
    context: &specs::Context,
    driver: &Driver,
    environment: &std::collections::BTreeMap<String, String>,
    arguments: &[String],
) -> Result<App, String> {
    let executable =
        common::executable(context, "path to the Stado native application executable")?;
    let app = driver.launch_process(&executable, environment, arguments)?;
    let view = wait_for_screen(
        driver,
        app.pid,
        app.window_id,
        |tree| Regex::new(r"AX\w*Button \(Posture").unwrap().is_match(tree),
        "/AX\\w*Button \\(Posture/",
        Duration::from_secs(60),
    )?;
    for needle in ["Connect to Stado", "This source cannot be read"] {
        if view.tree.contains(needle) {
            driver.quit_app(app.pid);
            return Err(format!("the console has no configured Stado source, so no screen can load fleet state: the screen shows {needle:?}"));
        }
    }
    Ok(app)
}

pub fn open_screen<L>(
    driver: &Driver,
    pid: u32,
    window_id: u64,
    title: &str,
    loaded: L,
    loaded_description: &str,
    failures: &[&str],
    refresh: &str,
    timeout: Duration,
) -> Result<View, String>
where
    L: Fn(&str) -> bool,
{
    click(driver, pid, window_id, title)?;
    let answered =
        |tree: &str| loaded(tree) || failures.iter().any(|failure| tree.contains(failure));
    let mut view = wait_for_screen(
        driver,
        pid,
        window_id,
        answered,
        loaded_description,
        timeout,
    )?;
    let mut refusals = Vec::new();
    for _ in 0..3 {
        if loaded(&view.tree) {
            break;
        }
        let state = failures
            .iter()
            .find(|failure| view.tree.contains(**failure))
            .copied();
        refusals.push(
            state
                .and_then(|label| find_button(&view, label).map(|button| button.label))
                .or_else(|| state.map(str::to_string))
                .unwrap_or_else(|| "an unreadable state".to_string()),
        );
        click(driver, pid, window_id, refresh)?;
        view = poll(driver, pid, window_id, &loaded, timeout)?
            .unwrap_or(read_window(driver, pid, window_id)?);
    }
    if !loaded(&view.tree) {
        return Err(format!(
            "{title} never reached {loaded_description} in 4 reads; the screen answered: {}",
            common::tail(&refusals.join(" || "), 1500)
        ));
    }
    Ok(view)
}

pub fn select_row<F>(
    driver: &Driver,
    pid: u32,
    window_id: u64,
    needle: F,
    description: &str,
    timeout: Duration,
    minimum_fields: usize,
    skip: usize,
) -> Result<(View, Button), String>
where
    F: Fn(&str) -> bool,
{
    let first = read_window(driver, pid, window_id)?;
    let rows = row_buttons(&first, minimum_fields);
    if rows.len() <= skip {
        return Err(format!(
            "the table shows no selectable row past {skip}; buttons on screen: {}",
            buttons(&first)
                .into_iter()
                .map(|item| item.label)
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    let mut tried = Vec::new();
    for row in rows.into_iter().skip(skip) {
        match click(driver, pid, window_id, &row.label) {
            Ok(result) => {
                let code = refusal(&result).unwrap_or_default();
                tried.push(if code.is_empty() {
                    row.label.clone()
                } else {
                    format!("{} ({code})", row.label)
                });
            }
            Err(error) => {
                tried.push(format!(
                    "{} (gone: {})",
                    row.label,
                    common::tail(&error, 120)
                ));
                continue;
            }
        }
        if let Some(answered) = poll(driver, pid, window_id, &needle, timeout)? {
            return Ok((answered, row));
        }
    }
    Err(format!(
        "no row selection produced {description}; rows tried: {}",
        if tried.is_empty() {
            "none".to_string()
        } else {
            tried.join(" | ")
        }
    ))
}

pub fn attempt(driver: &Driver, pid: u32, window_id: u64, label: &str) -> Value {
    match click(driver, pid, window_id, label) {
        Ok(result) => result,
        Err(error) => serde_json::json!({ "code": "error", "result": common::tail(&error, 400) }),
    }
}
