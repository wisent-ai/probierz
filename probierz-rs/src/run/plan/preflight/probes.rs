use crate::run::*;
use serde_json::json;
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
    // Only a missing manifest is a driver Appium has not registered; one
    // that cannot be read or parsed is said with its cause, so the preflight
    // does not send the operator to reinstall a driver that is there.
    let file = home.join("node_modules/.cache/appium/extensions.yaml");
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(error) => {
            eprintln!("cannot read {}: {error}", file.display());
            return false;
        }
    };
    let document = match serde_yaml::from_str::<serde_yaml::Value>(&text) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("{} is not YAML: {error}", file.display());
            return false;
        }
    };
    document
        .get("drivers")
        .and_then(|drivers| drivers.get(name))
        .and_then(|driver| driver.get("pkgName"))
        .and_then(serde_yaml::Value::as_str)
        == Some(&format!("appium-{name}-driver"))
}

/// The simulators `xcrun simctl` lists. A simctl that fails or answers
/// something other than JSON is said on stderr with its cause, so "no iOS
/// simulators found" is not read as a machine without runtimes.
pub(crate) fn simctl_devices() -> Value {
    let result = capture_text(
        "xcrun",
        &["simctl", "list", "devices", "available", "--json"],
        None,
    );
    if !result.status.is_some_and(|status| status.success()) {
        eprintln!(
            "xcrun simctl list devices failed (status {:?}, spawn error {:?}): {}",
            result.status,
            result.error,
            String::from_utf8_lossy(&result.stderr).trim()
        );
        return json!({});
    }
    match serde_json::from_slice(&result.stdout) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("xcrun simctl list devices answered no JSON: {error}");
            json!({})
        }
    }
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
    let result = capture_text("who", &[], None);
    result.status.is_some_and(|status| status.success())
        && text(&result.stdout)
            .lines()
            .any(|line| line.split_whitespace().any(|part| part == "console"))
}

pub(crate) fn mac_automation_mode() -> bool {
    if std::env::consts::OS != "macos" {
        return false;
    }
    let result = capture_text("/usr/bin/automationmodetool", &[], None);
    result.status.is_some_and(|status| status.success())
        && text(&result.stdout)
            .to_ascii_lowercase()
            .contains("does not require user authentication")
}

pub(crate) fn check_row(
    name: impl Into<String>,
    ok: bool,
    own: bool,
    hint: impl Into<String>,
) -> Value {
    json!({ "name": name.into(), "ok": ok, "own": own, "hint": hint.into() })
}

pub(crate) fn env_snapshot(extra: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = std::env::vars().collect();
    env.extend(extra.clone());
    env
}
