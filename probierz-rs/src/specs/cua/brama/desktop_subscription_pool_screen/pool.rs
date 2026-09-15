//! What the loaded Subscription Pool table must say.
//!
//! The screen is read once and every assertion runs against that one
//! tree, so a failure names what was wrong with the screen a person
//! would have been looking at.

use super::*;

/// Columns the table must render.
const COLUMNS: [&str; 5] = [
    "PROVIDER",
    "SUBSCRIPTION",
    "STATE",
    "EXPIRES",
    "LAST REDEEM ERROR",
];

/// Texts that mean the screen is in a state this journey's ledger
/// rules out — still reading, failed to read, or empty.
const FORBIDDEN: [(&str, &str); 6] = [
    (
        "AXStaticText = \"The subscription pool could not be read\"",
        "the pool read should not have failed",
    ),
    (
        "AXStaticText = \"Reading the subscription pool\"",
        "the screen should not still be reading once the pool is on it",
    ),
    (
        "AXStaticText = \"Not read yet\"",
        "the screen should report when it read the pool",
    ),
    (
        "AXStaticText = \"Reading…\"",
        "no read should still be in flight",
    ),
    (
        "AXStaticText = \"The pool holds no subscription\"",
        "the ledger pool is not empty",
    ),
    (
        "AXStaticText = \"No subscription in the pool is live\"",
        "one ledger subscription is live",
    ),
];

/// One of each state, which is what the fixture's ledger holds.
const COUNTS: [(&str, usize); 4] = [("Live", 1), ("Burnt", 1), ("Expired", 1), ("Unknown", 1)];

/// The unusable rows must sort above the live one, because an operator
/// opens this screen to find what is broken.
const UNUSABLE: [&str; 3] = ["google", "mistral", "openai"];

pub(crate) fn assert_loaded_pool(loaded: &cua::Snapshot) -> Result<(), String> {
    let texts: HashSet<String> = common::static_texts(&loaded.tree).into_iter().collect();

    for (text, message) in [
        ("Subscription Pool", "the screen should render its title"),
        (
            "brama CLI",
            "the screen should scope itself to the CLI it read",
        ),
    ] {
        if !texts.contains(text) {
            return Err(message.to_string());
        }
    }
    for (needle, message) in FORBIDDEN {
        if loaded.tree.contains(needle) {
            return Err(message.to_string());
        }
    }
    if !texts.iter().any(|text| text.starts_with("read ")) {
        return Err("the screen should show the read's freshness".to_string());
    }
    for column in COLUMNS {
        if !texts.contains(column) {
            return Err(format!("the table should render the {column} column"));
        }
    }
    for (id, provider, state, error) in EXPECTED {
        let row = format!("({provider}, {id}, {state}, ");
        if !loaded.tree.contains(&row) {
            return Err(format!(
                "the table should render {provider} as {state} with its identity"
            ));
        }
        if error.is_some_and(|error| !loaded.tree.contains(error)) {
            return Err(format!(
                "the table should render the provider's own refusal for {provider}"
            ));
        }
    }
    if !loaded.tree.contains("No expiry recorded") {
        return Err(
            "a pooled subscription whose credential states no expiry should say so".to_string(),
        );
    }
    for (signal, count) in COUNTS {
        if !texts.contains(&format!("{signal}: {count}")) {
            return Err(format!(
                "the pool should count {count} {} subscription",
                signal.to_lowercase()
            ));
        }
    }

    let position = |provider: &str| {
        loaded
            .tree
            .find(&format!("({provider}, "))
            .unwrap_or(usize::MAX)
    };
    for unusable in UNUSABLE {
        if position(unusable) >= position("anthropic") {
            return Err(format!(
                "the unusable {unusable} row should sort above the live one"
            ));
        }
    }
    assert_no_secret(&loaded.tree, "the loaded pool")
}
