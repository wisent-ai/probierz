//! What the loaded Capabilities table must say.
//!
//! The screen's job is to tell an operator which capability routes
//! cannot be resolved and why, so the assertions are about exactly
//! that: the counts, the per-route resolution, the two distinct
//! reasons, and the order that puts the broken routes first.

use super::*;

/// Columns the table must render.
const COLUMNS: [&str; 4] = ["RESOURCE", "ITEM", "FIELD", "RESOLVES"];

/// Texts that mean the screen is in a state this fixture rules out.
const FORBIDDEN: [(&str, &str); 4] = [
    (
        "AXStaticText = \"Reading capability routes\"",
        "the screen should not still be loading once the routes are on it",
    ),
    (
        "AXStaticText = \"No capability routes\"",
        "the fixture table is not empty",
    ),
    (
        "AXStaticText = \"not read\"",
        "the screen should have read the table",
    ),
    (
        "Verifying routes against the vault",
        "no verification should be in flight",
    ),
];

pub(crate) fn assert_loaded_routes(loaded: &cua::Snapshot) -> Result<(), String> {
    let texts: HashSet<String> = common::static_texts(&loaded.tree).into_iter().collect();
    assert_static(&texts, "Capabilities", "the screen should render its title")?;
    if !loaded.tree.contains("(every consumer)") {
        return Err("the screen should say which consumer it read routes for".to_string());
    }
    if !loaded.tree.contains("AXButton (Read routes)") {
        return Err("the screen should render its read control".to_string());
    }
    assert_static(
        &texts,
        "3 routes, 2 unresolved",
        "the context bar should count the table and the routes that do not resolve",
    )?;
    for (needle, message) in FORBIDDEN {
        if loaded.tree.contains(needle) {
            return Err(message.to_string());
        }
    }
    for column in COLUMNS {
        if !loaded.tree.contains(&format!("AXButton \"{column}\"")) {
            return Err(format!("the table should render the {column} column"));
        }
    }
    for (resource, item, field, resolution) in FIXTURE_ROUTES {
        assert_static(
            &texts,
            resource,
            &format!("the table should render the route for {resource}"),
        )?;
        assert_static(
            &texts,
            item,
            &format!("the table should render the item {item} for {resource}"),
        )?;
        assert_static(
            &texts,
            field,
            &format!("the table should render the field {field} for {resource}"),
        )?;
        if !loaded.tree.contains(&format!("({resolution})")) {
            return Err(format!(
                "the table should resolve {resource} as {resolution}"
            ));
        }
    }
    assert_unresolved_reasons(loaded)?;
    assert_broken_routes_first(loaded)
}

/// The two ways a route fails are different problems and must be
/// reported separately.
fn assert_unresolved_reasons(loaded: &cua::Snapshot) -> Result<(), String> {
    if !loaded
        .tree
        .contains("One route names a field its item does not carry")
    {
        return Err("a route whose item lacks the named field should be called out".to_string());
    }
    if !loaded
        .tree
        .contains("One route names an item this host cannot read")
    {
        return Err(
            "a route whose item this host cannot read should be called out separately".to_string(),
        );
    }
    Ok(())
}

/// An operator opens this screen to find what is broken, so the broken
/// routes sort first, and the harder failure sorts above the softer.
fn assert_broken_routes_first(loaded: &cua::Snapshot) -> Result<(), String> {
    let position = |resource: &str| {
        loaded
            .tree
            .find(&format!("AXStaticText = \"{resource}\""))
            .unwrap_or(usize::MAX)
    };
    if position("https://sso.example.com") >= position("https://absent.example.com") {
        return Err(
            "the field-missing route should sort above the unreadable-item route".to_string(),
        );
    }
    if position("https://absent.example.com") >= position("https://login.example.com") {
        return Err("unresolved routes should sort above the route that resolves".to_string());
    }
    Ok(())
}
