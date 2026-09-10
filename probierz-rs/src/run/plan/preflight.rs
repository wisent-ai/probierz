use serde_json::json;
use crate::run::*;
pub(crate) fn appium_driver_installed(name: &str, env: &BTreeMap<String, String>) -> bool {
    let home = env
        .get("APPIUM_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".appium"));
    if !home
        .join("node_modules")
        .join(format!("appium-{name}-driver"))
        .exists()
    {
        return false;
    }
    let file = home.join("node_modules/.cache/appium/extensions.yaml");
    let Ok(document) = fs::read_to_string(file)
        .ok()
        .and_then(|value| serde_yaml::from_str::<serde_yaml::Value>(&value).ok())
        .ok_or(())
    else {
        return false;
    };
    document
        .get("drivers")
        .and_then(|drivers| drivers.get(name))
        .and_then(|driver| driver.get("pkgName"))
        .and_then(serde_yaml::Value::as_str)
        == Some(&format!("appium-{name}-driver"))
}

pub(crate) fn playwright_browsers_installed(harness: &Path) -> bool {
    let code = "const {chromium,firefox,webkit}=require('playwright');process.exit([chromium,firefox,webkit].every(b=>{const p=b.executablePath();return p&&require('fs').existsSync(p)})?0:1)";
    capture_text("node", &["-e", code], Some(harness), Some(PROBE_MS))
        .status
        .is_some_and(|status| status.success())
}

pub(crate) fn pkg_installed(harness: &Path, relative: &str) -> bool {
    harness.join("node_modules").join(relative).exists()
        || harness.join(relative).join("node_modules").exists()
}

pub(crate) fn simctl_devices() -> Value {
    let result = capture_text(
        "xcrun",
        &["simctl", "list", "devices", "available", "--json"],
        None,
        Some(PROBE_MS),
    );
    if !result.status.is_some_and(|status| status.success()) {
        return json!({});
    }
    serde_json::from_slice(&result.stdout).unwrap_or_else(|_| json!({}))
}

pub(crate) fn available_ios_runtimes() -> Vec<String> {
    let document = simctl_devices();
    let Some(devices) = document.get("devices").and_then(Value::as_object) else {
        return Vec::new();
    };
    let expression = Regex::new(r"SimRuntime\.iOS-(\d+)-(\d+)").expect("literal regex");
    let mut found = BTreeSet::new();
    for (key, list) in devices {
        if !list.as_array().is_some_and(|list| !list.is_empty()) {
            continue;
        }
        if let Some(groups) = expression.captures(key) {
            found.insert(format!("{}.{}", &groups[1], &groups[2]));
        }
    }
    found.into_iter().collect()
}

pub(crate) fn available_ios_devices() -> Vec<String> {
    let document = simctl_devices();
    let mut found = BTreeSet::new();
    if let Some(devices) = document.get("devices").and_then(Value::as_object) {
        for list in devices.values().filter_map(Value::as_array) {
            for device in list {
                if let Some(name) = device.get("name").and_then(Value::as_str) {
                    found.insert(name.to_string());
                }
            }
        }
    }
    found.into_iter().collect()
}

pub(crate) fn app_build_sdk(app: &str) -> Option<String> {
    let info = Path::new(app).join("Info.plist");
    let result = capture(
        "/usr/libexec/PlistBuddy",
        &[
            "-c".into(),
            "Print :DTPlatformVersion".into(),
            info.to_string_lossy().into_owned(),
        ],
        None,
        None,
        Some(PROBE_MS),
    );
    if !result.status.is_some_and(|status| status.success()) {
        return None;
    }
    let value = text(&result.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

pub(crate) fn cua_accessibility_granted() -> bool {
    let socket = home_dir().join("Library/Caches/cua-driver/probierz.sock");
    let result = capture(
        "cua-driver",
        &[
            "call".into(),
            "check_permissions".into(),
            "{\"prompt\":false}".into(),
            "--socket".into(),
            socket.to_string_lossy().into_owned(),
        ],
        None,
        None,
        Some(PROBE_MS),
    );
    if !result.status.is_some_and(|status| status.success()) {
        return false;
    }
    let document: Value = serde_json::from_slice(&result.stdout).unwrap_or(Value::Null);
    document
        .pointer("/permissions/accessibility")
        .or_else(|| document.get("accessibility"))
        .and_then(Value::as_bool)
        == Some(true)
}

pub(crate) fn has_console_session() -> bool {
    if std::env::consts::OS != "macos" {
        return false;
    }
    let result = capture_text("who", &[], None, Some(PROBE_MS));
    result.status.is_some_and(|status| status.success())
        && text(&result.stdout)
            .lines()
            .any(|line| line.split_whitespace().any(|part| part == "console"))
}

pub(crate) fn mac_automation_mode() -> bool {
    if std::env::consts::OS != "macos" {
        return false;
    }
    let result = capture_text("/usr/bin/automationmodetool", &[], None, Some(PROBE_MS));
    result.status.is_some_and(|status| status.success())
        && text(&result.stdout)
            .to_ascii_lowercase()
            .contains("does not require user authentication")
}

pub(crate) fn check_row(name: impl Into<String>, ok: bool, own: bool, hint: impl Into<String>) -> Value {
    json!({ "name": name.into(), "ok": ok, "own": own, "hint": hint.into() })
}

pub(crate) fn env_snapshot(extra: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = std::env::vars().collect();
    env.extend(extra.clone());
    env
}

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

pub(crate) fn setup_steps(harness: &Path, name: &str) -> Result<Vec<SetupStep>, Failure> {
    let npm = SetupStep {
        name: "npm install (workspaces)".into(),
        command: "npm".into(),
        args: vec!["install".into()],
        output_dir: None,
        skip_driver: None,
        skip_cua: false,
    };
    let pw = |pkg: &str, with_deps: bool| SetupStep {
        name: format!("playwright browsers ({pkg})"),
        command: "npm".into(),
        args: [
            "--workspace",
            &format!("packages/{pkg}"),
            "exec",
            "playwright",
            "install",
        ]
        .into_iter()
        .map(str::to_string)
        .chain(with_deps.then_some("--with-deps".into()))
        .collect(),
        output_dir: None,
        skip_driver: None,
        skip_cua: false,
    };
    let driver = |name: &str, version: Option<&str>| SetupStep {
        name: format!("appium driver: {name}"),
        command: "npx".into(),
        args: vec![
            "--no-install".into(),
            "appium".into(),
            "driver".into(),
            "install".into(),
            version
                .map(|version| format!("{name}@{version}"))
                .unwrap_or_else(|| name.into()),
        ],
        output_dir: None,
        skip_driver: Some(name.into()),
        skip_cua: false,
    };
    let native_binary = harness.join("node_modules/.cache/probierz/screen-capture-kit");
    let native = SetupStep {
        name: "ScreenCaptureKit recorder".into(),
        command: "xcrun".into(),
        args: vec![
            "swiftc".into(),
            "-parse-as-library".into(),
            harness
                .join("packages/desktop-native/tools/screen-capture-kit.swift")
                .to_string_lossy()
                .into_owned(),
            "-o".into(),
            native_binary.to_string_lossy().into_owned(),
        ],
        output_dir: native_binary.parent().map(Path::to_path_buf),
        skip_driver: None,
        skip_cua: false,
    };
    Ok(match name {
        "web" => vec![npm, pw("web", true)],
        "electron" => vec![npm, pw("electron", false)],
        "mobile:ios" => vec![npm, driver("xcuitest", None)],
        "mobile:android" => vec![npm, driver("uiautomator2", None)],
        "desktop:mac" => vec![npm, driver("mac2", Some("2.2.2")), native],
        "desktop:win" => vec![npm, driver("windows", None)],
        // The daemon this surface needs is started by the driver itself, in
        // `ensure_daemon`, so setup has nothing to install for it beyond the
        // workspace dependencies its report tooling shares.
        "desktop:cua" => vec![npm],
        "tui" => vec![npm],
        _ => {
            return Err(fail(
                "run.setup",
                format!("unknown target: {name} ({})", accepted_preflight_targets()),
            ))
        }
    })
}

pub fn setup(harness: &Path, name: &str, args: &[String]) -> Answer {
    if target(name).is_none() {
        return Err(fail("cli.setup", format!("unknown target: {name}")));
    }
    let opts = parse_run_args(args, false)?;
    let env = env_snapshot(&BTreeMap::new());
    let mut done = Vec::new();
    let mut failure: Option<(String, String)> = None;
    for step in setup_steps(harness, name)? {
        let line = format!("{} {}", step.command, step.args.join(" "));
        let skipped = step
            .skip_driver
            .as_ref()
            .is_some_and(|driver| appium_driver_installed(driver, &env))
            || (step.skip_cua && cua_accessibility_granted());
        if skipped {
            done.push(json!({ "step": step.name, "command": line, "ok": true, "skipped": true }));
            continue;
        }
        if let Some(directory) = &step.output_dir {
            fs::create_dir_all(directory)?;
        }
        let result = capture(
            &step.command,
            &step.args,
            Some(harness),
            None,
            Some(if opts.timeout_ms > 0 {
                opts.timeout_ms
            } else {
                30 * 60 * 1000
            }),
        );
        let ok = result.status.is_some_and(|status| status.success()) && !result.timed_out;
        let exit_code = result.status.and_then(|status| status.code()).unwrap_or(-1);
        done.push(json!({ "step": step.name, "command": line, "ok": ok, "exitCode": exit_code }));
        if !ok {
            let tail = tail_chars(&text(&result.stderr), 2000);
            failure = Some((step.name, tail));
            break;
        }
    }
    let ok = failure.is_none();
    let mut result = if let Some((step, stderr)) = failure {
        json!({ "target": name, "ok": false, "steps": done, "failedAt": step, "stderrTail": stderr })
    } else {
        json!({ "target": name, "ok": true, "steps": done })
    };
    result.as_object_mut().expect("object").insert(
        "preflight".into(),
        preflight(harness, name, &BTreeMap::new())?,
    );
    print_json(&result)?;
    if !ok {
        std::process::exit(1);
    }
    Ok(())
}

