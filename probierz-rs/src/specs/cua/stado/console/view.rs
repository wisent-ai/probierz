use super::*;
pub(crate) const POLL: Duration = Duration::from_millis(500);
pub(crate) const CONSOLE_CHROME: &[&str] = &[
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
    pub(crate) snapshot: Snapshot,
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

pub(crate) fn view_from(snapshot: Snapshot) -> View {
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

