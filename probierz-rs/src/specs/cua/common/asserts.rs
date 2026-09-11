//! What a CUA journey asserts about a window tree: text present, text absent, a
//! pattern matched, the static texts, and the tail of a long dump.

use regex::Regex;

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
