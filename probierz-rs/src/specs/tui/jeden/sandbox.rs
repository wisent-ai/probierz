//! A fresh HOME for one jeden session.
//!
//! `warm_cache` brings the Brama catalog cache along (structural journeys must
//! not pay a cold catalog fetch; latency journeys keep it cold on purpose);
//! `credentials` decides whether the operator's `.jeden/.env` comes with it.
//! The sandbox pins the interface language, because every journey reads the
//! English screens.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::specs::tui::common;

const SANDBOX_LANGUAGE: &str = "en";

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|error| format!("{}: {error}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|error| format!("{}: {error}", from.display()))? {
        let entry = entry.map_err(|error| format!("{}: {error}", from.display()))?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)
                .map_err(|error| format!("{}: {error}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Create the sandbox and return its path; the caller removes it with
/// `common::remove` when the session ends.
pub(crate) fn home(warm_cache: bool, credentials: bool) -> Result<PathBuf, String> {
    let operator = PathBuf::from(std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?)
        .join(".jeden");
    let sandbox = common::scratch("probierz-tui-home")?;
    let jeden = sandbox.join(".jeden");
    fs::create_dir_all(&jeden).map_err(|error| format!("{}: {error}", jeden.display()))?;
    let files: &[&str] = if credentials {
        &[".env", "config.yml"]
    } else {
        &["config.yml"]
    };
    for file in files {
        let source = operator.join(file);
        if source.is_file() {
            fs::copy(&source, jeden.join(file))
                .map_err(|error| format!("{}: {error}", source.display()))?;
        }
    }
    let cache = operator.join("cache");
    if warm_cache && cache.is_dir() {
        copy_tree(&cache, &jeden.join("cache"))?;
    }
    let config = jeden.join("config.yml");
    if let Ok(text) = fs::read_to_string(&config) {
        // A config that is not JSON keeps the operator's shape untouched.
        if let Ok(mut parsed @ Value::Object(_)) = serde_json::from_str::<Value>(&text) {
            if !parsed["ui"].is_object() {
                parsed["ui"] = Value::Object(serde_json::Map::new());
            }
            parsed["ui"]["language"] = Value::String(SANDBOX_LANGUAGE.into());
            fs::write(&config, parsed.to_string())
                .map_err(|error| format!("{}: {error}", config.display()))?;
        }
    }
    Ok(sandbox)
}
