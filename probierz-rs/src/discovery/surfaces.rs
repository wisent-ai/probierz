use crate::discovery::*;

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

pub(crate) const SPEC_DIRS: [&str; 3] = ["test/specs", "tests", "specs"];
pub(crate) const SPEC_SUFFIXES: [&str; 3] = [".e2e.ts", ".spec.ts", ".spec.mjs"];

/// The exact command that runs a target. It is returned as text: this product
/// prints it so an operator can run it, and `run` is the command that executes
/// one.
pub(crate) const RUN_COMMANDS: [(&str, &str); 8] = [
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
pub(crate) struct SurfaceSpecs {
    pub(crate) surface: String,
    pub(crate) specs: Vec<String>,
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

