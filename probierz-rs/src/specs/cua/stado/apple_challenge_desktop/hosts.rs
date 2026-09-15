//! The host this journey acts on: validating the name the operator
//! named, and selecting its one row on the Hosts screen.

use super::*;

/// Columns a Hosts row carries, used to read the row's buttons.
const HOST_ROW_COLUMNS: usize = 3;

/// The dedicated host the operator named, with no room for a name that
/// would select something else by accident.
pub(crate) fn declared_host(context: &specs::Context) -> Result<String, String> {
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
    Ok(host.to_string())
}

/// Open Hosts, wait for the real inventory, and click the single row
/// for this host. Refuses when the table holds no row or several.
pub(crate) fn select_host(
    driver: &crate::cua::Driver,
    app: &crate::cua::App,
    host: &str,
) -> Result<(), String> {
    console::open_screen(
        driver,
        app.pid,
        app.window_id,
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
        driver,
        app.pid,
        app.window_id,
        |tree| row_pattern.is_match(tree),
        &format!("/{}/", row_pattern.as_str()),
        GATES,
    )?;

    let rows = console::row_buttons(&hosts, HOST_ROW_COLUMNS);
    let matching = rows
        .iter()
        .filter(|row| row.label == host || row.label.starts_with(&format!("{host},")))
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        let listed = rows
            .iter()
            .map(|row| row.label.clone())
            .collect::<Vec<_>>()
            .join(" | ");
        return Err(format!(
            "the real Hosts table must contain exactly one row for {host:?}; rows: {}",
            if listed.is_empty() {
                "none".to_string()
            } else {
                listed
            }
        ));
    }
    console::click(driver, app.pid, app.window_id, &matching[0].label).map(|_| ())
}
