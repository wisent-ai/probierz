use super::*;
pub(crate) fn clean(text: &str) -> String {
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
