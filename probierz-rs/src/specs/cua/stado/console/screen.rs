use super::*;
pub(crate) fn element_of(view: &View, index: u64) -> Option<&Value> {
    view.elements
        .iter()
        .find(|element| element.get("element_index").and_then(Value::as_u64) == Some(index))
}

pub(crate) fn refusal(result: &Value) -> Option<&str> {
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

