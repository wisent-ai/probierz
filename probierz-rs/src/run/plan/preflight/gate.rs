use serde_json::json;
use crate::run::*;
pub(crate) fn preflight(
    harness: &Path,
    name: &str,
    extra: &BTreeMap<String, String>,
) -> Result<Value, Failure> {
    let env = env_snapshot(extra);
    let setup = |target: &str| format!("probierz setup {target}");
    let checks = match name {
        "web" | "electron" => vec![
            check_row("@playwright/test", pkg_installed(harness, "@playwright/test"), true, setup(name)),
            check_row("playwright browsers", playwright_browsers_installed(harness), true, setup(name)),
        ],
        "mobile:ios" => {
            let runtimes = available_ios_runtimes();
            let pinned = env.get("IOS_VERSION").cloned();
            let sim_ok = pinned.as_ref().map(|pin| runtimes.contains(pin)).unwrap_or(!runtimes.is_empty());
            let sim_name = pinned.as_ref().map(|pin| format!("iOS simulator runtime {pin}")).unwrap_or_else(|| "iOS simulator runtime (any)".into());
            let sim_hint = if let Some(pin) = &pinned {
                if runtimes.is_empty() { "no iOS simulator runtimes found; open Xcode > Settings > Platforms and add one".into() }
                else { format!("iOS {pin} runtime not installed. Available: {}. Set IOS_VERSION to one of these, or add {pin} via Xcode > Settings > Platforms.", runtimes.join(", ")) }
            } else { "no iOS simulator runtimes found; open Xcode > Settings > Platforms and add one".into() };
            let wanted = env.get("IOS_DEVICE").map(String::as_str).unwrap_or("iPhone 15");
            let devices = available_ios_devices();
            let device_hint = if devices.is_empty() { "no iOS simulators found; open Xcode > Settings > Platforms and add a runtime".into() }
                else { format!("simulator \"{wanted}\" not found. Available: {}. Set IOS_DEVICE to one of these.", devices.join(", ")) };
            let mut rows = vec![
                check_row("Xcode command-line tools (xcrun)", successful("xcrun", &["--version"]), false, "install Xcode from the App Store, then: xcode-select --install"),
                check_row("xcodebuild", successful("xcodebuild", &["-version"]), false, "install Xcode from the App Store"),
                check_row(sim_name, sim_ok, false, sim_hint),
                check_row(format!("simulator device \"{wanted}\""), devices.iter().any(|name| name == wanted), false, device_hint),
                check_row("appium driver: xcuitest", appium_driver_installed("xcuitest", &env), true, setup(name)),
            ];
            if pinned.is_none() {
                if let Some(app) = env.get("APP_IOS") {
                    if let Some(sdk) = app_build_sdk(app) {
                        if !runtimes.is_empty() && !runtimes.contains(&sdk) {
                            let suggest = runtimes.last().cloned().unwrap_or_default();
                            rows.push(check_row(format!("app build SDK iOS {sdk} installed"), false, false,
                                format!("APP_IOS was built against iOS {sdk}, which is not installed (have: {}). Without IOS_VERSION, XCUITest targets the build SDK and fails. Set IOS_VERSION={suggest} to force an installed runtime.", runtimes.join(", "))));
                        }
                    }
                }
            }
            rows
        }
        "mobile:android" => vec![
            check_row("adb", successful("adb", &["version"]), false, "install Android SDK platform-tools and add them to PATH"),
            check_row("ANDROID_HOME set", env.contains_key("ANDROID_HOME") || env.contains_key("ANDROID_SDK_ROOT"), false, "export ANDROID_HOME to your Android SDK location"),
            check_row("appium driver: uiautomator2", appium_driver_installed("uiautomator2", &env), true, setup(name)),
        ],
        "desktop:mac" => vec![
            check_row("macOS host", std::env::consts::OS == "macos", false, "the mac2 driver runs on macOS only"),
            check_row("full Xcode toolchain", successful("xcodebuild", &["-version"]), false, "install Xcode from the App Store and select it with xcode-select"),
            check_row("UI automation without authentication", mac_automation_mode(), false, "sudo /usr/bin/automationmodetool enable-automationmode-without-authentication"),
            check_row("appium driver: mac2", appium_driver_installed("mac2", &env), true, setup(name)),
        ],
        "desktop:win" => vec![
            check_row("Windows host", std::env::consts::OS == "windows", false, "WinAppDriver runs on Windows only"),
            check_row("WinAppDriver", successful("WinAppDriver.exe", &["--help"]), false, "install WinAppDriver from github.com/microsoft/WinAppDriver/releases"),
        ],
        // A journey needs a controlling terminal, which `script` provides. The
        // runner is this binary now, so neither node nor a python shim is a
        // prerequisite of the terminal surface any more.
        "tui" => vec![
            check_row("terminal session (script)", successful("script", &["-q", "/dev/null", "true"]), false, "script(1) gives a journey a controlling terminal; it ships with macOS and util-linux"),
        ],
        // The one readiness question that cannot be answered from a file: the
        // login mailbox has to open and say so, or the journey will sit waiting
        // for a code that never arrives.
        "mobile:ios:byk-auth" => {
            let (reachable, hint) = byk_mailbox_reachable(harness, &env);
            vec![
                check_row("Xcode command-line tools (xcrun)", successful("xcrun", &["--version"]), false, "install Xcode from the App Store, then: xcode-select --install"),
                check_row("appium driver: xcuitest", appium_driver_installed("xcuitest", &env), true, setup("mobile:ios")),
                check_row("app under test declared", env.contains_key("APP_IOS") != env.contains_key("BUNDLE_ID"), false, "set exactly one of APP_IOS or BUNDLE_ID"),
                check_row(format!("{BYK_MAILBOX} mailbox reachable"), reachable, false,
                    if hint.is_empty() { "the login mailbox answers".to_string() } else { hint }),
            ]
        }
        "desktop:cua" => vec![
            check_row("macOS host", std::env::consts::OS == "macos", false, "the cua-driver drives the macOS Accessibility API"),
            check_row("logged-in macOS console session", has_console_session(), false, "select a dedicated macOS host with an active GUI login session"),
            check_row("cua-driver binary", successful("cua-driver", &["--version"]), false, "install cua-driver (macOS Accessibility driver)"),
            check_row("cua-driver accessibility", cua_accessibility_granted(), true, "grant CuaDriver in System Settings > Privacy & Security > Accessibility (once per host)"),
        ],
        _ => return Err(fail("run.preflight", format!("unknown target: {name} ({})", accepted_preflight_targets()))),
    };
    let missing: Vec<Value> = checks
        .iter()
        .filter(|row| row.get("ok") != Some(&Value::Bool(true)))
        .filter_map(|row| row.get("name").cloned())
        .collect();
    let mut seen = BTreeSet::new();
    let remediation: Vec<Value> = checks
        .iter()
        .filter(|row| row.get("ok") != Some(&Value::Bool(true)))
        .filter_map(|row| row.get("hint").and_then(Value::as_str))
        .filter(|hint| seen.insert((*hint).to_string()))
        .map(|hint| Value::String(hint.to_string()))
        .collect();
    Ok(
        json!({ "target": name, "ready": missing.is_empty(), "checks": checks, "missing": missing, "remediation": remediation }),
    )
}

pub fn check(harness: &Path, name: &str) -> Answer {
    if target(name).is_none() {
        return Err(fail("cli.check", format!("unknown target: {name}")));
    }
    let result = preflight(harness, name, &BTreeMap::new())?;
    let ready = result
        .get("ready")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    print_json(&result)?;
    if !ready {
        std::process::exit(1);
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) struct SetupStep {
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) args: Vec<String>,
    pub(crate) output_dir: Option<PathBuf>,
    pub(crate) skip_driver: Option<String>,
    pub(crate) skip_cua: bool,
}

