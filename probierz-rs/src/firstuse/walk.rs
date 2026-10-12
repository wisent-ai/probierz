//! The path a new user walks through a product's first-use definition, and
//! the command each screen tells them to run.
//!
//! A user starts at `entry_screen_id` and follows each screen's transition to
//! the next. A journey run follows exactly one path, so a screen offering
//! several transitions, one naming a screen the definition lacks, or a path
//! that comes back to a screen already walked is refused naming the screen.
//! A screen's `presentation.command` is the line the user reads: its optional
//! parts (`[--json]`) are left out and every placeholder, an all-capital word
//! such as `PATH`, is the user's own data, declared in `firstUse.fixtures`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::Value as Json;

use crate::failure::Failure;

fn refuse(what: impl Into<String>) -> Failure {
    Failure::config("first-use.definition", what)
}

/// The screens in the order a user meets them.
pub(crate) fn path(definition: &Json) -> Result<Vec<&Json>, Failure> {
    let screens = definition["screens"]
        .as_array()
        .ok_or_else(|| refuse("the definition declares no screens array"))?;
    let by_id: BTreeMap<&str, &Json> = screens
        .iter()
        .filter_map(|screen| screen["screen_id"].as_str().map(|id| (id, screen)))
        .collect();
    let mut next = definition["entry_screen_id"]
        .as_str()
        .ok_or_else(|| refuse("the definition declares no entry_screen_id"))?;
    let mut walked: Vec<&Json> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    loop {
        let screen = *by_id.get(next).ok_or_else(|| {
            refuse(format!(
                "the path names screen {next}, which the definition does not declare"
            ))
        })?;
        if !seen.insert(next) {
            return Err(refuse(format!(
                "the path comes back to screen {next}; a first-use journey is walked once"
            )));
        }
        walked.push(screen);
        let transitions = screen["transitions"]
            .as_array()
            .ok_or_else(|| refuse(format!("screen {next} declares no transitions array")))?;
        match transitions.as_slice() {
            [] => return Ok(walked),
            [only] => {
                next = only["next_screen_id"]
                    .as_str()
                    .ok_or_else(|| refuse(format!("screen {next}'s transition names no next_screen_id")))?;
            }
            several => {
                return Err(refuse(format!(
                    "screen {next} offers {} transitions; a journey run follows one path, so the definition must offer one",
                    several.len()
                )))
            }
        }
    }
}

/// The line with every optional `[...]` part left out.
fn required_part(command: &str) -> String {
    let mut kept = String::new();
    let mut open: Vec<char> = Vec::new();
    for character in command.chars() {
        match character {
            '[' => open.push(character),
            ']' => {
                open.pop();
            }
            other if open.is_empty() => kept.push(other),
            _ => {}
        }
    }
    kept
}

fn placeholder(word: &str) -> bool {
    word.chars().any(|character| character.is_ascii_uppercase())
        && word
            .chars()
            .all(|character| character.is_ascii_uppercase() || character == '_')
}

/// The argv of the command a screen shows, its placeholders the declared
/// fixtures; `None` for a screen that shows no command.
pub(crate) fn argv(
    screen: &Json,
    fixtures: &BTreeMap<String, PathBuf>,
) -> Result<Option<Vec<String>>, Failure> {
    let Some(command) = screen["presentation"]["command"].as_str() else {
        return Ok(None);
    };
    let screen_id = &screen["screen_id"];
    let mut words = Vec::new();
    for word in required_part(command).split_whitespace() {
        if !placeholder(word) {
            words.push(word.to_string());
            continue;
        }
        let fixture = fixtures.get(word).ok_or_else(|| {
            refuse(format!(
                "screen {screen_id} tells the user to run `{command}`, and {word} is their own data; declare firstUse.fixtures.{word}"
            ))
        })?;
        words.push(fixture.to_string_lossy().into_owned());
    }
    if words.is_empty() {
        return Err(refuse(format!(
            "screen {screen_id}'s command `{command}` names no program"
        )));
    }
    Ok(Some(words))
}

/// The first-success fact and the value the product must record for it: the
/// `completion_evidence.value` of the screen whose evidence is that fact.
pub(crate) fn expected_first_success(definition: &Json) -> Result<(String, Json), Failure> {
    let fact = definition["first_success_fact"]
        .as_str()
        .ok_or_else(|| refuse("the definition declares no first_success_fact"))?
        .to_string();
    let evidence = definition["screens"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|screen| &screen["completion_evidence"])
        .find(|evidence| evidence["fact"].as_str() == Some(fact.as_str()))
        .ok_or_else(|| {
            refuse(format!(
                "no screen's completion_evidence names the first-success fact {fact}"
            ))
        })?;
    Ok((fact, evidence["value"].clone()))
}
