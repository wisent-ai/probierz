//! Read-only discovery: what exists, not what happened.
//!
//! Nothing here starts a driver, installs a dependency, executes a suite or
//! touches an application repository. Running a journey needs Chromium, Appium
//! or a simulator, and keeping that out of the read surface is why an operator
//! can ask these questions on any machine.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_yaml::Value;

use crate::failure::{print_json, Answer, Failure};
use crate::manifest;

/// The surfaces this toolkit drives, with the tool that owns each one. One
/// source of truth for `list`; the run commands are strings and are never
/// spawned from here.
#[derive(Debug, Clone, Serialize)]
pub struct Surface {
    pub name: &'static str,
    pub pkg: &'static str,
    pub tool: &'static str,
    pub script: &'static str,
    pub targets: &'static str,
    pub env: &'static [&'static str],
}

pub const SURFACES: &[Surface] = &[
    Surface {
        name: "web",
        pkg: "packages/web",
        tool: "Playwright",
        script: "test:web",
        targets: "Chromium / Firefox / WebKit + emulated mobile",
        env: &["BASE_URL"],
    },
    Surface {
        name: "electron",
        pkg: "packages/electron",
        tool: "Playwright (_electron)",
        script: "test:electron",
        targets: "Electron desktop app",
        env: &["ELECTRON_APP_MAIN"],
    },
    Surface {
        name: "mobile",
        pkg: "packages/mobile",
        tool: "WebdriverIO + Appium (XCUITest / UiAutomator2)",
        script: "test:mobile:ios | test:mobile:android",
        targets: "iOS / Android",
        env: &[
            "APP_IOS",
            "APP_ANDROID",
            "BUNDLE_ID",
            "APP_PACKAGE",
            "IOS_DEVICE",
            "IOS_VERSION",
            "GMAIL_TOKEN",
        ],
    },
    Surface {
        name: "desktop-native",
        pkg: "packages/desktop-native",
        tool: "WebdriverIO + Appium (Mac2 / WinAppDriver)",
        script: "test:desktop:mac | test:desktop:win",
        targets: "native macOS / Windows",
        env: &["MAC_BUNDLE_ID", "WIN_APP"],
    },
    Surface {
        name: "desktop-cua",
        pkg: "packages/desktop-cua",
        tool: "cua-driver",
        script: "test:desktop:cua",
        targets: "native desktop accessibility surfaces",
        env: &["CUA_APP_EXECUTABLE"],
    },
    Surface {
        name: "tui",
        pkg: "packages/tui",
        tool: "PTY",
        script: "test:tui",
        targets: "terminal applications",
        env: &["TUI_CMD"],
    },
];

const SPEC_DIRS: [&str; 3] = ["test/specs", "tests", "specs"];
const SPEC_SUFFIXES: [&str; 3] = [".e2e.ts", ".spec.ts", ".spec.mjs"];

/// The exact command that runs a target. It is returned as text: this product
/// prints it so an operator can run it, and `run` is the command that executes
/// one.
const RUN_COMMANDS: [(&str, &str); 8] = [
    ("web", "BASE_URL=https://example.com npm run test:web"),
    (
        "electron",
        "ELECTRON_APP_MAIN=/abs/app/main.js npm run test:electron",
    ),
    (
        "mobile:ios",
        "APP_IOS=/abs/App.app IOS_DEVICE='iPhone 17' npm run test:mobile:ios",
    ),
    (
        "mobile:android",
        "APP_ANDROID=/abs/app.apk npm run test:mobile:android",
    ),
    (
        "desktop:mac",
        "MAC_BUNDLE_ID=com.apple.TextEdit npm run test:desktop:mac",
    ),
    (
        "desktop:win",
        "WIN_APP='Microsoft.WindowsCalculator_8wekyb3d8bbwe!App' npm run test:desktop:win",
    ),
    ("desktop:cua", "probierz run desktop:cua"),
    ("tui", "probierz run tui"),
];

pub fn list(_harness: &Path) -> Answer {
    print_json(&SURFACES)
}

pub fn apps(harness: &Path) -> Answer {
    print_json(&manifest::list(harness)?)
}

pub fn app(harness: &Path, app_id: &str) -> Answer {
    let loaded = manifest::load(harness, app_id)?;
    let mut document = loaded.document.clone();
    if let Some(mapping) = document.as_mapping_mut() {
        mapping.insert(
            Value::from("file"),
            Value::from(loaded.file.to_string_lossy().into_owned()),
        );
    }
    let as_json: serde_json::Value = serde_yaml::from_value(document)?;
    print_json(&as_json)
}

#[derive(Debug, Serialize)]
struct SurfaceSpecs {
    surface: String,
    specs: Vec<String>,
}

pub fn specs(harness: &Path, surface: Option<&str>) -> Answer {
    let chosen: Vec<(&Surface, &str)> = match surface {
        Some(name) => {
            let package_name = if name == "desktop:cua" {
                "desktop-cua"
            } else {
                name
            };
            let found: Vec<(&Surface, &str)> = SURFACES
                .iter()
                .filter(|entry| entry.name == package_name)
                .map(|entry| (entry, name))
                .collect();
            if found.is_empty() {
                return Err(Failure::invalid(
                    "discovery.specs",
                    format!("unknown surface: {name}"),
                ));
            }
            found
        }
        None => SURFACES
            .iter()
            .map(|entry| {
                let name = if entry.name == "desktop-cua" {
                    "desktop:cua"
                } else {
                    entry.name
                };
                (entry, name)
            })
            .collect(),
    };
    let mut answer = Vec::with_capacity(chosen.len());
    for (entry, surface_name) in chosen {
        let specs = match surface_name {
            "tui" | "desktop:cua" => crate::specs::select(surface_name, None)
                .into_iter()
                .map(|spec| spec.title.to_string())
                .collect(),
            _ => spec_files(harness, entry.pkg)?,
        };
        answer.push(SurfaceSpecs {
            surface: surface_name.to_string(),
            specs,
        });
    }
    print_json(&answer)
}

fn spec_files(harness: &Path, pkg: &str) -> Result<Vec<String>, Failure> {
    let mut found = Vec::new();
    for sub in SPEC_DIRS {
        let directory = harness.join(pkg).join(sub);
        if !directory.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&directory)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if SPEC_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
                found.push(format!("{pkg}/{sub}/{name}"));
            }
        }
    }
    found.sort();
    Ok(found)
}

#[derive(Debug, Serialize)]
struct Outline {
    spec: String,
    count: usize,
    outline: Vec<OutlineEntry>,
}

#[derive(Debug, Serialize)]
struct OutlineEntry {
    kind: String,
    title: String,
}

/// The describe / it / test titles of one spec, in file order. A pure text
/// scan: a title is what the file says, never what a run reported.
pub fn describe(harness: &Path, spec: &str) -> Answer {
    let clean = spec.trim_start_matches('/').to_string();
    let absolute = harness.join(&clean);
    let resolved = absolute
        .canonicalize()
        .map_err(|_| Failure::invalid("discovery.describe", format!("spec not found: {clean}")))?;
    let root = harness.canonicalize()?;
    if !resolved.starts_with(&root) {
        return Err(Failure::invalid(
            "discovery.describe",
            "path escapes the probierz root",
        ));
    }
    let source = std::fs::read_to_string(&resolved)?;
    let outline = outline_of(&source);
    print_json(&Outline {
        spec: clean,
        count: outline.len(),
        outline,
    })
}

/// Titles are read with a scanner rather than a regular expression so a
/// quotation mark inside a title cannot end it early.
fn outline_of(source: &str) -> Vec<OutlineEntry> {
    let mut entries = Vec::new();
    let bytes = source.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let Some(kind) = ["describe", "it", "test"]
            .into_iter()
            .find(|kind| starts_call(source, bytes, index, kind))
        else {
            index += 1;
            continue;
        };
        let mut cursor = index + kind.len();
        while cursor < bytes.len() && (bytes[cursor] as char).is_whitespace() {
            cursor += 1;
        }
        // `describe(` — anything else is an identifier that merely starts the
        // same way.
        if cursor >= bytes.len() || bytes[cursor] != b'(' {
            index += kind.len();
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && (bytes[cursor] as char).is_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || !matches!(bytes[cursor], b'\'' | b'"' | b'`') {
            index = cursor;
            continue;
        }
        let quote = bytes[cursor];
        cursor += 1;
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor] != quote {
            if bytes[cursor] == b'\\' {
                cursor += 1;
            }
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }
        entries.push(OutlineEntry {
            kind: kind.to_string(),
            title: source[start..cursor].to_string(),
        });
        index = cursor + 1;
    }
    entries
}

fn starts_call(source: &str, bytes: &[u8], index: usize, kind: &str) -> bool {
    if !source[index..].starts_with(kind) {
        return false;
    }
    let before_is_word = index > 0
        && (bytes[index - 1].is_ascii_alphanumeric()
            || matches!(bytes[index - 1], b'_' | b'$' | b'.'));
    !before_is_word
}

#[derive(Debug, Serialize)]
struct RunCommand {
    target: String,
    command: String,
    note: &'static str,
}

pub fn cmd(_harness: &Path, target: &str) -> Answer {
    let Some((_, command)) = RUN_COMMANDS.iter().find(|(name, _)| *name == target) else {
        let known = RUN_COMMANDS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Failure::invalid(
            "discovery.cmd",
            format!("unknown target: {target} (one of {known})"),
        ));
    };
    print_json(&RunCommand {
        target: target.to_string(),
        command: (*command).to_string(),
        note: "read-only: this is the command to run yourself; probierz never executes it",
    })
}

/// One host a run can be placed on. The bridge reads the same inventory this
/// prints, so a selector cannot mean one thing to `hosts` and another to a
/// submission.
#[derive(Debug, Clone, Serialize)]
pub struct Host {
    pub host: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<&'static str>,
    #[serde(rename = "apiUrl", skip_serializing_if = "Option::is_none")]
    pub api_url: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<serde_json::Value>,
    pub description: &'static str,
}

impl Host {
    pub(crate) fn stado(host: &str, request: serde_json::Value, description: &'static str) -> Self {
        Self {
            host: host.to_string(),
            kind: "stado",
            platform: None,
            target: None,
            api_url: None,
            request: Some(request),
            description,
        }
    }
}

/// The hosts a run can be placed on, in the order an operator reads them.
///
/// One inventory, read by `hosts` and by the Stado bridge: a selector cannot
/// mean one placement when it is printed and another when it is submitted. A
/// selector constrains placement only — it never replaces the queue endpoint,
/// so `stado:mini` still submits through Stado's configured address.
pub fn host_inventory() -> Vec<Host> {
    let dedicated = |host: &str,
                     target: &'static str,
                     api_url: Option<&'static str>,
                     pinned: &str,
                     description: &'static str| Host {
        host: host.to_string(),
        kind: "stado",
        platform: Some("darwin"),
        target: Some(target),
        api_url,
        request: Some(serde_json::json!({
            "provider": "local",
            "pin_to_provider": true,
            "pinned_host": pinned,
        })),
        description,
    };
    vec![
        Host {
            host: "local".to_string(),
            kind: "local",
            platform: None,
            target: None,
            api_url: None,
            request: None,
            description: "this machine (default)",
        },
        Host::stado(
            "stado:gcp",
            serde_json::json!({ "provider": "gcp", "pin_to_provider": true }),
            "stado queue, GCP consumers only",
        ),
        Host::stado(
            "stado:azure",
            serde_json::json!({ "provider": "azure", "pin_to_provider": true }),
            "stado queue, Azure consumers only",
        ),
        Host::stado(
            "stado:aws",
            serde_json::json!({ "provider": "aws", "pin_to_provider": true }),
            "stado queue, AWS consumers only",
        ),
        Host::stado(
            "stado:any",
            serde_json::json!({}),
            "stado queue, any consumer with capacity",
        ),
        Host::stado(
            "stado:spot",
            serde_json::json!({ "max_cost_per_hour_usd": 4 }),
            "stado queue, cost-capped capacity",
        ),
        Host::stado(
            "stado:local",
            serde_json::json!({ "provider": "local", "pin_to_provider": true }),
            "stado queue, local-kind consumers only",
        ),
        dedicated(
            "stado:mini",
            "charless-mac-mini",
            None,
            "local-charless-mac-mini.local",
            "stado queue, dedicated Mac mini consumer",
        ),
        dedicated(
            "stado:macbook",
            "lukasz-macbook",
            Some("http://127.0.0.1:18765"),
            "local-lukaszs-macbook-pro-5485.local",
            "stado queue, dedicated MacBook consumer",
        ),
        Host::stado(
            "stado:t4",
            serde_json::json!({ "gpu_type": "nvidia-tesla-t4" }),
            "stado queue, nvidia-tesla-t4 capacity",
        ),
    ]
}

/// One host by its selector, or nothing when the selector is unknown.
pub fn stado_host(name: &str) -> Option<Host> {
    host_inventory()
        .into_iter()
        .find(|entry| entry.host == name)
}

pub fn hosts() -> Answer {
    print_json(&host_inventory())
}

/// Where a spec path resolves for a surface, used by run and author paths.
pub fn spec_dir(surface: &str) -> Option<PathBuf> {
    match surface {
        "web" => Some(PathBuf::from("packages/web/tests")),
        "electron" => Some(PathBuf::from("packages/electron/tests")),
        "mobile:ios" | "mobile:android" => Some(PathBuf::from("packages/mobile/test/specs")),
        "desktop:mac" | "desktop:win" => Some(PathBuf::from("packages/desktop-native/test/specs")),
        // `tui` and `desktop:cua` journeys are functions in this crate, listed
        // from the registry, so no directory holds them.
        "desktop:cua" | "tui" => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outline_keeps_a_quotation_mark_inside_a_title() {
        let source =
            "describe('a product\\'s journey', () => { it(\"reads `state`\", () => {}); });";
        let outline = outline_of(source);
        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].kind, "describe");
        assert_eq!(outline[0].title, "a product\\'s journey");
        assert_eq!(outline[1].kind, "it");
        assert_eq!(outline[1].title, "reads `state`");
    }

    #[test]
    fn an_identifier_that_merely_starts_like_a_title_call_is_not_one() {
        let outline = outline_of("const describeLater = 1; itemCount('x'); object.it('y');");
        assert!(outline.is_empty(), "unexpected outline: {outline:?}");
    }
}
