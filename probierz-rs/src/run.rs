//! Execution, preflight, report analysis, affected-target selection, CI orchestration,
//! and declared run matrices.
//!
//! The suite drivers remain the real product tools. This module only prepares
//! their environment, starts `npm run <script>` with the same argument vector as
//! the former Node runner, and turns the reports they write into durable facts.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, SecondsFormat, Utc};
use flate2::read::DeflateDecoder;
use regex::Regex;
use serde_json::{json, Map, Number, Value};
use sha2::{Digest, Sha256};
use url::Url;

use crate::failure::{fail, now_iso, print_json, Answer, Failure};
use crate::manifest;

const TAIL: usize = 4000;
const DEFAULT_TIMEOUT_MS: u64 = 20 * 60 * 1000;
const PROBE_MS: u64 = 15_000;
const SAMPLE_INTERVAL_MS: u64 = 1000;

#[derive(Clone, Copy)]
struct Target {
    pkg: &'static str,
    script: &'static str,
    tool: &'static str,
}

fn target(name: &str) -> Option<Target> {
    Some(match name {
        "web" => Target {
            pkg: "packages/web",
            script: "test:web",
            tool: "playwright",
        },
        "electron" => Target {
            pkg: "packages/electron",
            script: "test:electron",
            tool: "playwright",
        },
        "mobile:ios" => Target {
            pkg: "packages/mobile",
            script: "test:mobile:ios",
            tool: "wdio",
        },
        "mobile:ios:byk-auth" => Target {
            pkg: "packages/mobile",
            script: "test:mobile:ios:byk-auth",
            tool: "wdio",
        },
        "mobile:android" => Target {
            pkg: "packages/mobile",
            script: "test:mobile:android",
            tool: "wdio",
        },
        "desktop:mac" => Target {
            pkg: "packages/desktop-native",
            script: "test:desktop:mac",
            tool: "wdio",
        },
        "desktop:win" => Target {
            pkg: "packages/desktop-native",
            script: "test:desktop:win",
            tool: "wdio",
        },
        "desktop:cua" => Target {
            pkg: "packages/desktop-cua",
            script: "probierz run desktop:cua",
            tool: "cua-driver",
        },
        "tui" => Target {
            pkg: "packages/tui",
            script: "probierz run tui",
            tool: "probierz",
        },
        _ => return None,
    })
}

fn target_list() -> Vec<&'static str> {
    vec![
        "web",
        "electron",
        "mobile:ios",
        "mobile:ios:byk-auth",
        "mobile:android",
        "desktop:mac",
        "desktop:win",
        "desktop:cua",
        "tui",
    ]
}

fn accepted_preflight_targets() -> &'static str {
    "web|electron|mobile:ios|mobile:ios:byk-auth|mobile:android|desktop:mac|desktop:cua|desktop:win|tui"
}

/// The flags the six execution commands accept, printed by each of their
/// `--help` screens.
///
/// They share one parser, so they share one description. Help that omits a
/// flag the parser accepts is the same defect as a documented command the
/// binary does not have: the declaration stops matching the world.
/// The shared flag text, as a macro so that a command needing more can
/// `concat!` its own block onto it without a string-building dependency.
#[macro_export]
macro_rules! run_flags_help {
    () => {
        "\
Accepted arguments (parsed by the shared execution parser):
  NAME=VALUE            Environment variable given to the suite; repeatable
  --app <ID>            Application manifest whose surface and secrets apply
  --spec <FILE>         One spec file instead of the target's whole suite
  --tool <NAME>         Report shape to expect: playwright, wdio, or probierz
  --record              Keep video, traces, and screenshots for every journey
  --force               Run even when the resource this target locks is held
  --no-analyze          Skip report analysis and print the raw run
  --no-repair           Do not offer an authored repair for a failed run
  --frames <N>          Frames per second to extract from a recording
  --timeout <MS>        Give the suite this long before it is killed
  --resource-wait <MS>  Wait this long for a held resource before refusing
  --files <PATH>...     Changed files that select what runs (affected, ci)
  --host <SELECTOR>     mobile:ios:byk-auth only: the fleet host its suite
                        is placed on, from `probierz hosts`; default
                        stado:mini
  --local               mobile:ios:byk-auth only: run its suite on this
                        machine instead of the dedicated host
  --seed-resend         mobile:ios:byk-auth only: seed the login mailbox's
                        resend source and stop, running no journey"
    };
}

pub const RUN_FLAGS_HELP: &str = run_flags_help!();

/// The two flags only `matrix` accepts, appended to its own help.
/// The two flags only `matrix` accepts.
#[macro_export]
macro_rules! matrix_flags_help {
    () => {
        "\
Matrix-only arguments:
  --plan                Print the matrix this app and profile resolve to,
                        running nothing
  --release <ID>        The release the matrix runs against; required when the
                        profile is `release` and the matrix is executed"
    };
}

#[derive(Default)]
struct RunArgs {
    env: BTreeMap<String, String>,
    record: bool,
    analyze: bool,
    force: bool,
    no_repair: bool,
    app_id: Option<String>,
    spec: Option<String>,
    frames: f64,
    timeout_ms: u64,
    resource_wait_ms: Option<u64>,
    tool: Option<String>,
    /// Run the byk-auth worker on this machine instead of the dedicated host.
    local: bool,
    /// The fleet host the remote byk-auth suite is placed on.
    host: Option<String>,
    /// Seed the login mailbox's resend source and stop, without a journey.
    seed_resend: bool,
}

fn parse_non_negative(flag: &str, value: &str) -> Result<f64, Failure> {
    let parsed = value.parse::<f64>().map_err(|_| {
        fail(
            "cli.arguments",
            format!("{flag} needs a non-negative number"),
        )
    })?;
    if !parsed.is_finite() || parsed < 0.0 {
        return Err(fail(
            "cli.arguments",
            format!("{flag} needs a non-negative number"),
        ));
    }
    Ok(parsed)
}

fn value_after(args: &[String], index: usize, flag: &str) -> Result<String, Failure> {
    let value = args.get(index + 1).filter(|value| !value.starts_with("--"));
    value
        .cloned()
        .ok_or_else(|| fail("cli.arguments", format!("{flag} needs a value")))
}

fn parse_run_args(args: &[String], allow_positionals: bool) -> Result<RunArgs, Failure> {
    let mut opts = RunArgs {
        analyze: true,
        ..RunArgs::default()
    };
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--record" => opts.record = true,
            "--force" => opts.force = true,
            "--no-repair" => opts.no_repair = true,
            "--no-analyze" => opts.analyze = false,
            "--frames" => {
                let value = value_after(args, index, arg)?;
                opts.frames = parse_non_negative(arg, &value)?;
                index += 1;
            }
            "--timeout" => {
                let value = value_after(args, index, arg)?;
                opts.timeout_ms = parse_non_negative(arg, &value)? as u64;
                index += 1;
            }
            "--resource-wait" => {
                let value = value_after(args, index, arg)?;
                opts.resource_wait_ms = Some(parse_non_negative(arg, &value)? as u64);
                index += 1;
            }
            "--spec" => {
                opts.spec = Some(value_after(args, index, arg)?);
                index += 1;
            }
            "--app" => {
                opts.app_id = Some(value_after(args, index, arg)?);
                index += 1;
            }
            "--tool" => {
                opts.tool = Some(value_after(args, index, arg)?);
                index += 1;
            }
            "--local" => opts.local = true,
            "--host" => { opts.host = Some(value_after(args, index, arg)?); index += 1; }
            "--seed-resend" => opts.seed_resend = true,
            "--files" => {}
            _ if arg.starts_with("--") => {
                return Err(fail("cli.arguments", format!("unknown option: {arg}")))
            }
            _ if arg.contains('=') => {
                let (name, value) = arg.split_once('=').unwrap_or((arg, ""));
                opts.env.insert(name.to_string(), value.to_string());
            }
            _ if !allow_positionals => {
                return Err(fail("cli.arguments", format!("unexpected argument: {arg}")))
            }
            _ => {}
        }
        index += 1;
    }
    Ok(opts)
}

fn files_after_flag(args: &[String]) -> Option<Vec<String>> {
    let start = args.iter().position(|arg| arg == "--files")? + 1;
    let valued = [
        "--frames",
        "--timeout",
        "--resource-wait",
        "--spec",
        "--app",
        "--tool",
    ];
    let mut files = Vec::new();
    let mut index = start;
    while index < args.len() {
        if valued.contains(&args[index].as_str()) {
            index += 2;
        } else {
            if !args[index].starts_with("--") && !args[index].contains('=') {
                files.push(args[index].clone());
            }
            index += 1;
        }
    }
    Some(files)
}

struct Captured {
    status: Option<ExitStatus>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    error: Option<String>,
    timed_out: bool,
}

fn terminate_tree(child: &mut std::process::Child, hard: bool) {
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        command.args(["/PID", &child.id().to_string(), "/T"]);
        if hard {
            command.arg("/F");
        }
        let _ = command.stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(not(windows))]
    {
        let signal = if hard { "-KILL" } else { "-TERM" };
        let group = format!("-{}", child.id());
        let _ = Command::new("/bin/kill")
            .args([signal, &group])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !hard {
            let _ = child.kill();
        }
    }
}

fn capture(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    env: Option<&BTreeMap<String, String>>,
    timeout_ms: Option<u64>,
) -> Captured {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    if let Some(env) = env {
        command.envs(env);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Captured {
                status: None,
                stdout: Vec::new(),
                stderr: Vec::new(),
                error: Some(error.to_string()),
                timed_out: false,
            }
        }
    };
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(ref mut stream) = stdout {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    });
    let err_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(ref mut stream) = stderr {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    });
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if timeout_ms
                    .is_some_and(|timeout| started.elapsed() >= Duration::from_millis(timeout))
                {
                    timed_out = true;
                    terminate_tree(&mut child, false);
                    thread::sleep(Duration::from_millis(25));
                    if child.try_wait().ok().flatten().is_none() {
                        terminate_tree(&mut child, true);
                    }
                    break child.wait().ok();
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_error) => {
                break {
                    let _ = child.kill();
                    None
                }
            }
        }
    };
    Captured {
        status,
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
        error: None,
        timed_out,
    }
}

fn capture_text(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: Option<u64>,
) -> Captured {
    capture(
        program,
        &args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>(),
        cwd,
        None,
        timeout_ms,
    )
}

fn successful(program: &str, args: &[&str]) -> bool {
    capture_text(program, args, None, Some(PROBE_MS))
        .status
        .is_some_and(|status| status.success())
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn appium_driver_installed(name: &str, env: &BTreeMap<String, String>) -> bool {
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

fn playwright_browsers_installed(harness: &Path) -> bool {
    let code = "const {chromium,firefox,webkit}=require('playwright');process.exit([chromium,firefox,webkit].every(b=>{const p=b.executablePath();return p&&require('fs').existsSync(p)})?0:1)";
    capture_text("node", &["-e", code], Some(harness), Some(PROBE_MS))
        .status
        .is_some_and(|status| status.success())
}

fn pkg_installed(harness: &Path, relative: &str) -> bool {
    harness.join("node_modules").join(relative).exists()
        || harness.join(relative).join("node_modules").exists()
}

fn simctl_devices() -> Value {
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

fn available_ios_runtimes() -> Vec<String> {
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

fn available_ios_devices() -> Vec<String> {
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

fn app_build_sdk(app: &str) -> Option<String> {
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

fn cua_accessibility_granted() -> bool {
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

fn has_console_session() -> bool {
    if std::env::consts::OS != "macos" {
        return false;
    }
    let result = capture_text("who", &[], None, Some(PROBE_MS));
    result.status.is_some_and(|status| status.success())
        && text(&result.stdout)
            .lines()
            .any(|line| line.split_whitespace().any(|part| part == "console"))
}

fn mac_automation_mode() -> bool {
    if std::env::consts::OS != "macos" {
        return false;
    }
    let result = capture_text("/usr/bin/automationmodetool", &[], None, Some(PROBE_MS));
    result.status.is_some_and(|status| status.success())
        && text(&result.stdout)
            .to_ascii_lowercase()
            .contains("does not require user authentication")
}

fn check_row(name: impl Into<String>, ok: bool, own: bool, hint: impl Into<String>) -> Value {
    json!({ "name": name.into(), "ok": ok, "own": own, "hint": hint.into() })
}

fn env_snapshot(extra: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = std::env::vars().collect();
    env.extend(extra.clone());
    env
}

fn preflight(
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
struct SetupStep {
    name: String,
    command: String,
    args: Vec<String>,
    output_dir: Option<PathBuf>,
    skip_driver: Option<String>,
    skip_cua: bool,
}

fn setup_steps(harness: &Path, name: &str) -> Result<Vec<SetupStep>, Failure> {
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

fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

fn slash(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    fn go(
        p: &[u8],
        v: &[u8],
        memo: &mut HashMap<(usize, usize), bool>,
        pi: usize,
        vi: usize,
    ) -> bool {
        if let Some(answer) = memo.get(&(pi, vi)) {
            return *answer;
        }
        let answer = if pi == p.len() {
            vi == v.len()
        } else if p[pi] == b'*' && pi + 1 < p.len() && p[pi + 1] == b'*' {
            go(p, v, memo, pi + 2, vi) || (vi < v.len() && go(p, v, memo, pi, vi + 1))
        } else if p[pi] == b'*' {
            go(p, v, memo, pi + 1, vi)
                || (vi < v.len() && v[vi] != b'/' && go(p, v, memo, pi, vi + 1))
        } else {
            vi < v.len() && p[pi] == v[vi] && go(p, v, memo, pi + 1, vi + 1)
        };
        memo.insert((pi, vi), answer);
        answer
    }
    go(
        pattern.as_bytes(),
        value.as_bytes(),
        &mut HashMap::new(),
        0,
        0,
    )
}

fn yaml_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(value) => Some(value.clone()),
        serde_yaml::Value::Number(value) => Some(value.to_string()),
        serde_yaml::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn affected_app_journeys(harness: &Path, files: &[String]) -> Result<Vec<Value>, Failure> {
    let mut matches = Vec::new();
    for app in manifest::list(harness)? {
        let declaration = manifest::load(harness, &app.app_id)?;
        let document = &declaration.document;
        let mut journey_targets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        if let Some(surfaces) = document
            .get("surfaces")
            .and_then(serde_yaml::Value::as_mapping)
        {
            for (target, surface) in surfaces {
                let Some(target) = target.as_str() else {
                    continue;
                };
                for journey in surface
                    .get("journeys")
                    .and_then(serde_yaml::Value::as_sequence)
                    .into_iter()
                    .flatten()
                    .filter_map(serde_yaml::Value::as_str)
                {
                    journey_targets
                        .entry(journey.into())
                        .or_default()
                        .insert(target.into());
                }
            }
        }
        for repository in document
            .get("repositories")
            .and_then(serde_yaml::Value::as_sequence)
            .into_iter()
            .flatten()
        {
            let Some(root) = repository.get("root").and_then(serde_yaml::Value::as_str) else {
                continue;
            };
            let root_path = Path::new(root);
            for input in files {
                let input_path = Path::new(input);
                let absolute = if input_path.is_absolute() {
                    normalize_path(input_path)
                } else {
                    normalize_path(&root_path.join(input_path))
                };
                let Ok(relative) = absolute.strip_prefix(root_path) else {
                    continue;
                };
                let relative = slash(relative);
                for mapping in repository
                    .get("mappings")
                    .and_then(serde_yaml::Value::as_sequence)
                    .into_iter()
                    .flatten()
                {
                    let patterns: Vec<&str> = mapping
                        .get("paths")
                        .and_then(serde_yaml::Value::as_sequence)
                        .into_iter()
                        .flatten()
                        .filter_map(serde_yaml::Value::as_str)
                        .collect();
                    if !patterns
                        .iter()
                        .any(|pattern| glob_matches(pattern, &relative))
                    {
                        continue;
                    }
                    let journeys: Vec<String> = mapping
                        .get("journeys")
                        .and_then(serde_yaml::Value::as_sequence)
                        .into_iter()
                        .flatten()
                        .filter_map(serde_yaml::Value::as_str)
                        .map(str::to_string)
                        .collect();
                    let targets: BTreeSet<String> = journeys
                        .iter()
                        .flat_map(|journey| {
                            journey_targets.get(journey).into_iter().flatten().cloned()
                        })
                        .collect();
                    matches.push(json!({ "appId": app.app_id, "input": input, "file": absolute.to_string_lossy(), "repository": root, "journeys": journeys, "targets": targets }));
                }
            }
        }
    }
    Ok(matches)
}

fn path_inside(parent: &Path, child: &Path) -> bool {
    normalize_path(child)
        .strip_prefix(normalize_path(parent))
        .is_ok()
}

fn affected_targets(harness: &Path, files: &[String]) -> Result<Value, Failure> {
    let app_matches = affected_app_journeys(harness, files)?;
    let mut by_package: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for name in target_list() {
        by_package
            .entry(target(name).expect("known").pkg)
            .or_default()
            .push(name);
    }
    let mut hit = BTreeSet::new();
    let mut cross_cutting = false;
    let mut classified = Vec::new();
    for raw in files {
        let product: Vec<&Value> = app_matches
            .iter()
            .filter(|entry| entry.get("input").and_then(Value::as_str) == Some(raw))
            .collect();
        if !product.is_empty() {
            let targets: BTreeSet<String> = product
                .iter()
                .flat_map(|entry| {
                    entry
                        .get("targets")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                })
                .collect();
            hit.extend(targets.clone());
            let apps: Vec<Value> = product.iter().map(|entry| json!({ "appId": entry["appId"], "journeys": entry["journeys"], "repository": entry["repository"] })).collect();
            classified.push(json!({ "file": slash(&normalize_path(Path::new(raw))), "affects": targets, "apps": apps }));
            continue;
        }
        let normalized = normalize_path(Path::new(raw));
        let package = by_package
            .iter()
            .find(|(pkg, _)| path_inside(Path::new(pkg), &normalized));
        if let Some((_pkg, names)) = package {
            for name in names {
                hit.insert((*name).to_string());
            }
            classified.push(json!({ "file": slash(&normalized), "affects": names }));
        } else if path_inside(Path::new("agent"), &normalized)
            || normalized.parent() == Some(Path::new(""))
        {
            cross_cutting = true;
            classified.push(json!({ "file": slash(&normalized), "affects": "all" }));
        } else {
            classified.push(json!({ "file": slash(&normalized), "affects": [] }));
        }
    }
    let targets: Vec<String> = if cross_cutting {
        target_list()
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    } else {
        hit.into_iter().collect()
    };
    let apps: Vec<Value> = app_matches
        .into_iter()
        .map(|mut entry| {
            entry.as_object_mut().expect("object").shift_remove("input");
            entry
        })
        .collect();
    Ok(
        json!({ "targets": targets, "crossCutting": cross_cutting, "files": classified, "apps": apps }),
    )
}

fn changed_files(harness: &Path, reference: &str) -> Result<Vec<String>, Failure> {
    let result = capture(
        "git",
        &[
            "-C".into(),
            harness.to_string_lossy().into_owned(),
            "diff".into(),
            "--name-only".into(),
            reference.into(),
        ],
        None,
        None,
        None,
    );
    if !result.status.is_some_and(|status| status.success()) {
        return Err(fail(
            "run.affected",
            format!(
                "git diff --name-only {reference} failed: {}",
                text(&result.stderr).trim()
            ),
        ));
    }
    Ok(text(&result.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn affected_from_git(harness: &Path, reference: Option<&str>) -> Result<Value, Failure> {
    let against = reference.unwrap_or("HEAD");
    let files = changed_files(harness, against)?;
    let mut result = affected_targets(harness, &files)?;
    let mut object = Map::new();
    object.insert("ref".into(), Value::String(against.into()));
    object.extend(result.as_object_mut().expect("object").clone());
    Ok(Value::Object(object))
}

pub fn affected(harness: &Path, args: &[String]) -> Answer {
    let result = if let Some(files) = files_after_flag(args) {
        if files.is_empty() {
            return Err(fail("cli.affected", "--files needs at least one path"));
        }
        affected_targets(harness, &files)?
    } else {
        let reference = args
            .first()
            .filter(|arg| !arg.starts_with("--"))
            .map(String::as_str);
        affected_from_git(harness, reference)?
    };
    print_json(&result)
}

fn walk(root: &Path, sort: bool) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }
    if root.is_file() {
        return vec![root.to_path_buf()];
    }
    let mut entries: Vec<_> = fs::read_dir(root).into_iter().flatten().flatten().collect();
    if sort {
        entries.sort_by_key(|entry| entry.file_name());
    }
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path, sort));
        } else {
            files.push(path);
        }
    }
    files
}

fn size_kb(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|meta| (meta.len() + 512) / 1024)
        .unwrap_or(0)
}

fn has_media_binary(name: &str) -> bool {
    successful(name, &["-version"])
}

fn probe_video(file: &Path) -> Option<Value> {
    if !has_media_binary("ffprobe") {
        return None;
    }
    let result = capture(
        "ffprobe",
        &[
            "-v".into(),
            "error".into(),
            "-select_streams".into(),
            "v:0".into(),
            "-show_entries".into(),
            "stream=width,height:format=duration".into(),
            "-of".into(),
            "json".into(),
            file.to_string_lossy().into_owned(),
        ],
        None,
        None,
        None,
    );
    if !result.status.is_some_and(|status| status.success()) || result.stdout.is_empty() {
        return None;
    }
    let document: Value = serde_json::from_slice(&result.stdout).ok()?;
    let stream = document
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| streams.first())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let duration = document
        .pointer("/format/duration")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .map(number)
        .unwrap_or(Value::Null);
    Some(
        json!({ "durationSec": duration, "width": stream.get("width").cloned().unwrap_or(Value::Null), "height": stream.get("height").cloned().unwrap_or(Value::Null) }),
    )
}

fn extract_frames(video: &Path, artifacts: &Path, count: f64) -> Vec<PathBuf> {
    let stem = video
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let output = artifacts.join("frames").join(stem);
    let _ = fs::remove_dir_all(&output);
    if !has_media_binary("ffmpeg") {
        return Vec::new();
    }
    let _ = fs::create_dir_all(&output);
    let duration = probe_video(video)
        .and_then(|meta| meta.get("durationSec").and_then(Value::as_f64))
        .unwrap_or(0.0);
    let n = count.max(1.0);
    let fps = if duration > 0.0 { n / duration } else { 1.0 };
    let pattern = output.join("frame_%03d.png");
    let result = capture(
        "ffmpeg",
        &[
            "-y".into(),
            "-i".into(),
            video.to_string_lossy().into_owned(),
            "-vf".into(),
            format!("fps={fps}"),
            pattern.to_string_lossy().into_owned(),
        ],
        None,
        None,
        None,
    );
    if !result.status.is_some_and(|status| status.success()) {
        return Vec::new();
    }
    walk(&output, true)
}

fn number(value: f64) -> Value {
    if !value.is_finite() {
        return Value::Null;
    }
    if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        return Value::Number(Number::from(value as i64));
    }
    Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}
fn js_number(value: Option<&Value>) -> f64 {
    value
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or(0.0)
}

fn normalize_wdio(report: &Value, tool: &str) -> Value {
    let rows = report
        .get("tests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut tests = Vec::new();
    let mut failures = Vec::new();
    let mut media = Vec::new();
    for row in &rows {
        let status = row
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                if row.get("passed").and_then(Value::as_bool).unwrap_or(false) {
                    "passed".into()
                } else {
                    "failed".into()
                }
            });
        let duration = js_number(row.get("duration"));
        let mut test = Map::new();
        test.insert(
            "title".into(),
            row.get("title").cloned().unwrap_or(Value::Null),
        );
        test.insert("passed".into(), Value::Bool(status == "passed"));
        test.insert("status".into(), Value::String(status.clone()));
        test.insert("durationMs".into(), number(duration));
        if let Some(video) = row.get("video") {
            test.insert("video".into(), video.clone());
        }
        tests.push(Value::Object(test));
        if status == "failed" {
            let error = row
                .get("error")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .unwrap_or("failed");
            failures.push(json!({ "title": row.get("title").cloned().unwrap_or(Value::Null), "error": error }));
        }
        if let Some(items) = row.get("media").and_then(Value::as_array) {
            media.extend(items.clone());
        }
        if let Some(video) = row.get("video").filter(|value| !value.is_null()) {
            media.push(json!({ "file": video, "kind": "video" }));
        }
    }
    let count = |wanted: &str| {
        tests
            .iter()
            .filter(|test| test.get("status").and_then(Value::as_str) == Some(wanted))
            .count()
    };
    let duration: f64 = tests
        .iter()
        .map(|test| js_number(test.get("durationMs")))
        .sum();
    json!({ "tool": tool, "total": report.get("total").cloned().unwrap_or_else(|| json!(tests.len())), "passed": count("passed"), "failed": count("failed"), "flaky": number(js_number(report.get("flaky"))), "skipped": count("skipped"), "durationMs": number(duration), "tests": tests, "failures": failures, "reportMedia": media })
}
fn first_nonzero(primary: Option<&Value>, fallback: Option<&Value>) -> f64 {
    let primary = js_number(primary);
    if primary != 0.0 {
        primary
    } else {
        js_number(fallback)
    }
}

fn visit_playwright(
    suite: &Value,
    tests: &mut Vec<Value>,
    failures: &mut Vec<Value>,
    media: &mut Vec<Value>,
) {
    for spec in suite
        .get("specs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mut statuses = Vec::new();
        let mut duration = 0.0;
        let mut error: Option<String> = None;
        for test in spec
            .get("tests")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for result in test
                .get("results")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                statuses.push(
                    result
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                );
                duration += js_number(result.get("duration"));
                for attachment in result
                    .get("attachments")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if attachment.get("path").and_then(Value::as_str).is_some() {
                        let mut item = Map::new();
                        item.insert("file".into(), attachment["path"].clone());
                        item.insert(
                            "kind".into(),
                            attachment.get("name").cloned().unwrap_or(Value::Null),
                        );
                        if let Some(content_type) = attachment.get("contentType") {
                            item.insert("contentType".into(), content_type.clone());
                        }
                        media.push(Value::Object(item));
                    }
                }
                if result.get("status").and_then(Value::as_str) != Some("passed") {
                    if let Some(first) = result
                        .get("errors")
                        .and_then(Value::as_array)
                        .and_then(|errors| errors.first())
                    {
                        error = Some(
                            first
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .lines()
                                .next()
                                .unwrap_or("")
                                .to_string(),
                        );
                    }
                }
            }
        }
        let status = if spec.get("ok").and_then(Value::as_bool) == Some(false) {
            "failed"
        } else if !statuses.is_empty() && statuses.iter().all(|status| status == "skipped") {
            "skipped"
        } else {
            "passed"
        };
        tests.push(json!({ "title": spec.get("title").cloned().unwrap_or(Value::Null), "passed": status == "passed", "status": status, "durationMs": number(duration) }));
        if status == "failed" {
            failures.push(json!({ "title": spec.get("title").cloned().unwrap_or(Value::Null), "error": error.filter(|value| !value.is_empty()).unwrap_or_else(|| "failed".into()) }));
        }
    }
    for child in suite
        .get("suites")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        visit_playwright(child, tests, failures, media);
    }
}

fn normalize_playwright(report: &Value) -> Value {
    let mut tests = Vec::new();
    let mut failures = Vec::new();
    let mut media = Vec::new();
    for suite in report
        .get("suites")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        visit_playwright(suite, &mut tests, &mut failures, &mut media);
    }
    let count = |wanted: &str| {
        tests
            .iter()
            .filter(|test| test.get("status").and_then(Value::as_str) == Some(wanted))
            .count()
    };
    json!({ "tool": "playwright", "total": tests.len(), "passed": count("passed"), "failed": count("failed"), "flaky": number(js_number(report.pointer("/stats/flaky"))), "skipped": count("skipped"), "durationMs": number(js_number(report.pointer("/stats/duration"))), "tests": tests, "failures": failures, "reportMedia": media })
}

fn parse_iso(value: Option<&str>, fallback: &str) -> String {
    value
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|date| {
            date.with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        })
        .unwrap_or_else(|| fallback.to_string())
}

fn safe_message(value: &str) -> String {
    let mut result = Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]+")
        .expect("regex")
        .replace_all(value, "Bearer [REDACTED]")
        .into_owned();
    result = Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b")
        .expect("regex")
        .replace_all(&result, "[EMAIL]")
        .into_owned();
    Regex::new(r"(?i)((?:auth|cookie|credential|key|otp|password|secret|session|token)[A-Z0-9_.-]*\s*[=:]\s*)[^\s,;]+").expect("regex").replace_all(&result, "$1[REDACTED]").into_owned()
}

fn safe_url(value: &str) -> String {
    if let Ok(mut url) = Url::parse(value) {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_fragment(None);
        let sensitive = Regex::new(
            r"(?i)(auth|code|cookie|credential|email|key|otp|password|secret|session|token)",
        )
        .expect("regex");
        let source: Vec<(String, String)> = url
            .query_pairs()
            .map(|(name, _)| {
                let replacement = if sensitive.is_match(&name) {
                    "[REDACTED]"
                } else {
                    "[VALUE]"
                };
                (name.into_owned(), replacement.into())
            })
            .collect();
        if !source.is_empty() {
            url.query_pairs_mut().clear().extend_pairs(source);
        }
        let segments: Vec<String> = url.path().split('/').map(str::to_string).collect();
        let mut safe = Vec::new();
        for (index, segment) in segments.iter().enumerate() {
            let decoded = percent_decode(segment);
            let previous = index
                .checked_sub(1)
                .map(|at| percent_decode(&segments[at]))
                .unwrap_or_default();
            if decoded.contains('@')
                || (decoded.len() >= 32
                    && decoded
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'))
                || (sensitive.is_match(&previous) && !decoded.is_empty())
            {
                safe.push("%5BREDACTED%5D".into());
            } else {
                safe.push(segment.clone());
            }
        }
        url.set_path(&safe.join("/"));
        return url
            .to_string()
            .replace("%255B", "%5B")
            .replace("%255D", "%5D");
    }
    Regex::new(r"([?&][^=]+)=([^&\s]+)")
        .expect("regex")
        .replace_all(value, "$1=[VALUE]")
        .into_owned()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn zip_entries(file: &Path) -> Result<Vec<(String, String)>, String> {
    let buffer = fs::read(file).map_err(|error| error.to_string())?;
    if buffer.len() < 22 {
        return Err("zip end record missing".into());
    }
    let minimum = buffer.len().saturating_sub(65_557);
    let mut end = None;
    for offset in (minimum..=buffer.len() - 22).rev() {
        if read_u32(&buffer, offset) == Some(0x06054b50) {
            end = Some(offset);
            break;
        }
    }
    let end = end.ok_or("zip end record missing")?;
    let count = read_u16(&buffer, end + 10).ok_or("invalid zip end record")? as usize;
    let mut offset = read_u32(&buffer, end + 16).ok_or("invalid zip end record")? as usize;
    let mut entries = Vec::new();
    for _ in 0..count {
        if read_u32(&buffer, offset) != Some(0x02014b50) {
            return Err("invalid zip central directory".into());
        }
        let method = read_u16(&buffer, offset + 10).ok_or("invalid zip central directory")?;
        let size = read_u32(&buffer, offset + 20).ok_or("invalid zip central directory")? as usize;
        let name_len =
            read_u16(&buffer, offset + 28).ok_or("invalid zip central directory")? as usize;
        let extra_len =
            read_u16(&buffer, offset + 30).ok_or("invalid zip central directory")? as usize;
        let comment_len =
            read_u16(&buffer, offset + 32).ok_or("invalid zip central directory")? as usize;
        let local = read_u32(&buffer, offset + 42).ok_or("invalid zip central directory")? as usize;
        let name = String::from_utf8_lossy(
            buffer
                .get(offset + 46..offset + 46 + name_len)
                .ok_or("invalid zip central directory")?,
        )
        .into_owned();
        if read_u32(&buffer, local) != Some(0x04034b50) {
            return Err("invalid zip local header".into());
        }
        let local_name = read_u16(&buffer, local + 26).ok_or("invalid zip local header")? as usize;
        let local_extra = read_u16(&buffer, local + 28).ok_or("invalid zip local header")? as usize;
        let data_at = local + 30 + local_name + local_extra;
        let compressed = buffer
            .get(data_at..data_at + size)
            .ok_or("invalid zip data")?;
        let content = if method == 0 {
            Some(compressed.to_vec())
        } else if method == 8 {
            let mut decoded = Vec::new();
            DeflateDecoder::new(compressed)
                .read_to_end(&mut decoded)
                .map_err(|error| error.to_string())?;
            Some(decoded)
        } else {
            None
        };
        if let Some(content) = content {
            entries.push((name, String::from_utf8_lossy(&content).into_owned()));
        }
        offset += 46 + name_len + extra_len + comment_len;
    }
    Ok(entries)
}
fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}
fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
fn json_lines(content: &str) -> Vec<Value> {
    content
        .lines()
        .filter_map(|line| {
            (!line.trim().is_empty())
                .then(|| serde_json::from_str(line).ok())
                .flatten()
        })
        .collect()
}

fn trace_events(file: &Path, fallback: &str, diagnostics: &mut Vec<Value>) -> Vec<Value> {
    let parsed = (|| -> Result<Vec<Value>, String> {
        let entries = zip_entries(file)?;
        let traces: Vec<Value> = entries
            .iter()
            .filter(|(name, _)| name.ends_with(".trace"))
            .flat_map(|(_, content)| json_lines(content))
            .collect();
        let context = traces.iter().find(|row| {
            row.get("type").and_then(Value::as_str) == Some("context-options")
                && js_number(row.get("wallTime")) != 0.0
                && js_number(row.get("monotonicTime")) != 0.0
        });
        let wall = context
            .map(|row| js_number(row.get("wallTime")))
            .unwrap_or(0.0);
        let monotonic = context
            .map(|row| js_number(row.get("monotonicTime")))
            .unwrap_or(0.0);
        let at_for = |value: f64| {
            if wall != 0.0 && value != 0.0 {
                DateTime::from_timestamp_millis((wall + value - monotonic) as i64)
                    .map(|date| date.to_rfc3339_opts(SecondsFormat::Millis, true))
                    .unwrap_or_else(|| fallback.into())
            } else {
                fallback.into()
            }
        };
        let source = file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let mut events = Vec::new();
        for row in entries
            .iter()
            .filter(|(name, _)| name.ends_with(".network"))
            .flat_map(|(_, content)| json_lines(content))
        {
            if row.get("type").and_then(Value::as_str) != Some("resource-snapshot") {
                continue;
            }
            let Some(url) = row.pointer("/snapshot/request/url").and_then(Value::as_str) else {
                continue;
            };
            let snapshot = &row["snapshot"];
            let status = js_number(snapshot.pointer("/response/status"));
            let duration = js_number(snapshot.get("time"));
            events.push(json!({ "at": at_for(first_nonzero(snapshot.get("_monotonicTime"), row.get("monotonicTime"))), "type": "network", "source": source, "method": snapshot.pointer("/request/method").cloned().unwrap_or(Value::Null), "url": safe_url(url), "status": if status == 0.0 { Value::Null } else { number(status) }, "durationMs": if duration == 0.0 { Value::Null } else { number(duration) } }));
        }
        for row in &traces {
            let params = row.get("params").unwrap_or(row);
            let method = row
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            let is_console = row.get("type").and_then(Value::as_str) == Some("console")
                || matches!(method.as_str(), "console" | "pageerror" | "page-error");
            if !is_console {
                continue;
            }
            let message = params
                .get("text")
                .or_else(|| params.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let severity = if method.contains("error") {
                "error"
            } else {
                params
                    .get("type")
                    .or_else(|| params.get("messageType"))
                    .and_then(Value::as_str)
                    .unwrap_or("log")
            };
            events.push(json!({ "at": at_for(first_nonzero(row.get("time"), row.get("monotonicTime"))), "type": "console", "source": source, "severity": severity, "message": safe_message(message).chars().take(2000).collect::<String>() }));
        }
        Ok(events)
    })();
    match parsed {
        Ok(events) => events,
        Err(error) => {
            diagnostics.push(json!({ "artifact": file, "error": error }));
            Vec::new()
        }
    }
}

fn json_trace_events(file: &Path, fallback: &str, diagnostics: &mut Vec<Value>) -> Vec<Value> {
    let result = (|| -> Result<Value, String> {
        let document: Value =
            serde_json::from_slice(&fs::read(file).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        if document.get("schemaVersion").and_then(Value::as_u64) != Some(1)
            || !document
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.starts_with("probierz-"))
            || document.get("status").and_then(Value::as_str) != Some("completed")
        {
            return Err("invalid Probierz JSON trace".into());
        }
        Ok(document)
    })();
    match result {
        Ok(document) => vec![
            json!({ "at": parse_iso(document.get("completedAt").and_then(Value::as_str), fallback), "type": "observation", "source": file.file_name().and_then(|name| name.to_str()).unwrap_or(""), "status": document["status"], "message": safe_message(document.pointer("/observation/reply").and_then(Value::as_str).unwrap_or("")).chars().take(2000).collect::<String>() }),
        ],
        Err(error) => {
            diagnostics.push(json!({ "artifact": file, "error": error }));
            Vec::new()
        }
    }
}

fn log_events(file: &Path, source: &str) -> Vec<Value> {
    let Ok(content) = fs::read_to_string(file) else {
        return Vec::new();
    };
    let fallback = modified_iso(file);
    let expression = Regex::new(r"^(\d{4}-\d{2}-\d{2}T\S+)\s(.*)$").expect("regex");
    content.lines().filter_map(|line| { let groups = expression.captures(line)?; Some(json!({ "at": parse_iso(Some(&groups[1]), &fallback), "type": "log", "source": source, "message": safe_message(&groups[2]) })) }).collect()
}

fn modified_iso(path: &Path) -> String {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .map(system_time_iso)
        .unwrap_or_else(now_iso)
}
fn system_time_iso(time: SystemTime) -> String {
    let millis = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    DateTime::from_timestamp_millis(millis)
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn build_timeline(
    report: &Value,
    summary: &Value,
    media: &[Value],
    artifacts: &Path,
    started: Option<&str>,
) -> Value {
    let mut diagnostics = Vec::new();
    let fallback = parse_iso(started, &now_iso());
    let mut events = Vec::new();
    events.extend(log_events(&artifacts.join("stdout.log"), "stdout"));
    events.extend(log_events(&artifacts.join("stderr.log"), "stderr"));
    let mut cursor = DateTime::parse_from_rfc3339(&fallback)
        .map(|date| date.timestamp_millis())
        .unwrap_or_else(|_| Utc::now().timestamp_millis());
    for test in report
        .get("tests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let duration = js_number(test.get("duration").or_else(|| test.get("durationMs")));
        let default_at = DateTime::from_timestamp_millis(cursor)
            .unwrap_or_else(Utc::now)
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let at = parse_iso(test.get("startedAt").and_then(Value::as_str), &default_at);
        let start_ms = DateTime::parse_from_rfc3339(&at)
            .map(|date| date.timestamp_millis())
            .unwrap_or(cursor);
        let default_completed = DateTime::from_timestamp_millis(start_ms + duration as i64)
            .unwrap_or_else(Utc::now)
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let completed = parse_iso(
            test.get("completedAt").and_then(Value::as_str),
            &default_completed,
        );
        events.push(json!({ "at": at, "completedAt": completed, "durationMs": duration, "type": "assertion", "source": summary.get("tool").cloned().unwrap_or(Value::Null), "title": test.get("title").cloned().unwrap_or(Value::Null), "status": test.get("status").cloned().unwrap_or_else(|| Value::String(if test.get("passed").and_then(Value::as_bool).unwrap_or(false) { "passed" } else { "failed" }.into())), "error": test.get("error").cloned().unwrap_or(Value::Null) }));
        cursor = DateTime::parse_from_rfc3339(&completed)
            .map(|date| date.timestamp_millis())
            .unwrap_or(cursor);
    }
    for item in media {
        let file = PathBuf::from(item.get("file").and_then(Value::as_str).unwrap_or(""));
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
        let at = if file.exists() {
            modified_iso(&file)
        } else {
            fallback.clone()
        };
        events.push(json!({ "at": at, "type": if kind == "screenshot" { "screenshot" } else { kind }, "source": summary.get("tool").cloned().unwrap_or(Value::Null), "artifact": file, "missing": item.get("missing").and_then(Value::as_bool).unwrap_or(false) }));
        if kind == "trace" && file.exists() {
            if item.get("contentType").and_then(Value::as_str) == Some("application/json") {
                events.extend(json_trace_events(&file, &at, &mut diagnostics));
            } else {
                events.extend(trace_events(&file, &at, &mut diagnostics));
            }
        }
    }
    events.sort_by(|left, right| {
        left.get("at")
            .and_then(Value::as_str)
            .cmp(&right.get("at").and_then(Value::as_str))
            .then(
                left.get("type")
                    .and_then(Value::as_str)
                    .cmp(&right.get("type").and_then(Value::as_str)),
            )
    });
    let mut counts = Map::new();
    let types: BTreeSet<String> = events
        .iter()
        .filter_map(|event| {
            event
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    for kind in types {
        counts.insert(
            kind.clone(),
            json!(events
                .iter()
                .filter(|event| event.get("type").and_then(Value::as_str) == Some(&kind))
                .count()),
        );
    }
    json!({ "schemaVersion": 1, "runId": report.pointer("/probierz/runId").cloned().unwrap_or(Value::Null), "artifactsDir": artifacts, "generatedAt": now_iso(), "counts": counts, "diagnostics": diagnostics, "events": events })
}

fn percentile(mut values: Vec<f64>, fraction: f64) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    let index = ((values.len() as f64 * fraction).ceil() as usize)
        .saturating_sub(1)
        .min(values.len() - 1);
    number(values[index])
}
fn error_line(value: &str) -> bool {
    Regex::new(r"(?i)\b(?:crash(?:ed)?|fatal|panic|uncaught|unhandled|segmentation fault|assertion failed)\b").expect("regex").is_match(value)
}

fn summarize_diagnostics(report: &Value, timeline: &Value, artifacts: &Path) -> Value {
    let assertions: Vec<f64> = report
        .get("tests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|test| js_number(test.get("duration").or_else(|| test.get("durationMs"))))
        .filter(|value| value.is_finite())
        .collect();
    let events = timeline
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let network: Vec<&Value> = events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("network"))
        .collect();
    let mut crashes: Vec<Value> = events.iter().filter(|event| event.get("type").and_then(Value::as_str) == Some("log") && error_line(event.get("message").and_then(Value::as_str).unwrap_or(""))).map(|event| json!({ "at": event["at"], "source": event["source"], "message": event.get("message").and_then(Value::as_str).unwrap_or("").chars().take(500).collect::<String>() })).collect();
    let directory = artifacts.join("diagnostics");
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("log") {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path) {
                for line in content.lines().filter(|line| error_line(line)) {
                    crashes.push(json!({ "source": path.file_name().and_then(|value| value.to_str()).unwrap_or(""), "message": safe_message(line).chars().take(500).collect::<String>() }));
                }
            }
        }
    }
    let network_errors: Vec<Value> = network.iter().filter(|event| js_number(event.get("status")) >= 400.0).map(|event| json!({ "at": event["at"], "method": event["method"], "url": event["url"], "status": event["status"] })).collect();
    let console_errors: Vec<Value> = events.iter().filter(|event| event.get("type").and_then(Value::as_str) == Some("console") && matches!(event.get("severity").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref(), Some("assert" | "error"))).map(|event| json!({ "at": event["at"], "severity": event["severity"], "message": safe_message(event.get("message").and_then(Value::as_str).unwrap_or("")).chars().take(500).collect::<String>() })).collect();
    let durations: Vec<f64> = network
        .iter()
        .map(|event| js_number(event.get("durationMs")))
        .filter(|value| *value > 0.0)
        .collect();
    let process = fs::read(artifacts.join("performance.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let process_summary = process.as_ref().map(|value| json!({ "firstOutputMs": value["firstOutputMs"], "peakRssKb": value["peakRssKb"], "averageCpuPercent": value["averageCpuPercent"], "appProcessName": value["appProcessName"], "appPeakRssKb": value["appPeakRssKb"], "appAverageCpuPercent": value["appAverageCpuPercent"] })).unwrap_or(Value::Null);
    let result = json!({ "schemaVersion": 1, "runId": report.pointer("/probierz/runId").cloned().unwrap_or(Value::Null), "crashes": crashes, "networkErrors": network_errors, "consoleErrors": console_errors, "performance": { "tests": { "count": assertions.len(), "p50Ms": percentile(assertions.clone(), 0.5), "p95Ms": percentile(assertions.clone(), 0.95), "maxMs": assertions.iter().copied().max_by(f64::total_cmp).map(number).unwrap_or(Value::Null) }, "network": { "count": durations.len(), "p50Ms": percentile(durations.clone(), 0.5), "p95Ms": percentile(durations.clone(), 0.95), "maxMs": durations.iter().copied().max_by(f64::total_cmp).map(number).unwrap_or(Value::Null) }, "process": process_summary } });
    let file = artifacts.join("diagnostics.json");
    let _ = write_json(&file, &result);
    let mut returned = Map::new();
    returned.insert("file".into(), json!(file));
    returned.extend(result.as_object().expect("object").clone());
    Value::Object(returned)
}

fn analyze_run(
    report_path: &Path,
    artifacts_dir: Option<&Path>,
    tool: Option<&str>,
    frames: f64,
    expected_run: Option<&str>,
) -> Result<Value, Failure> {
    if !report_path.exists() {
        return Err(fail(
            "run.analyze",
            format!(
                "report not found: {} (did the run produce one?)",
                report_path.display()
            ),
        ));
    }
    let report: Value = serde_json::from_slice(&fs::read(report_path)?)
        .map_err(|error| Failure::config("run.analyze", error.to_string()))?;
    let report_run = report.pointer("/probierz/runId").and_then(Value::as_str);
    if let Some(expected) = expected_run {
        if report_run != Some(expected) {
            return Err(fail(
                "run.analyze",
                format!(
                    "report run ID mismatch: expected {expected}, got {}",
                    report_run.unwrap_or("missing")
                ),
            ));
        }
    }
    let canonical =
        report.get("probierz").is_some() && report.get("tests").and_then(Value::as_array).is_some();
    let playwright = report.get("suites").and_then(Value::as_array).is_some();
    let mut summary = if canonical {
        normalize_wdio(&report, tool.unwrap_or("probierz"))
    } else if playwright {
        normalize_playwright(&report)
    } else {
        normalize_wdio(&report, tool.unwrap_or("wdio"))
    };
    let report_media = summary
        .as_object_mut()
        .expect("object")
        .remove("reportMedia")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let mut media = Vec::new();
    for item in report_media {
        let Some(file) = item.get("file").and_then(Value::as_str) else {
            continue;
        };
        let path = PathBuf::from(file);
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
        let mut entry = Map::new();
        entry.insert("file".into(), Value::String(file.into()));
        entry.insert("kind".into(), Value::String(kind.into()));
        if let Some(content_type) = item.get("contentType").filter(|value| !value.is_null()) {
            entry.insert("contentType".into(), content_type.clone());
        }
        if path.exists() {
            entry.insert("sizeKb".into(), json!(size_kb(&path)));
            if kind == "video" {
                if let Some(meta) = probe_video(&path) {
                    entry.insert("recording".into(), meta);
                }
                if frames > 0.0 {
                    if let Some(artifacts) = artifacts_dir {
                        entry.insert(
                            "frames".into(),
                            json!(extract_frames(&path, artifacts, frames)),
                        );
                    }
                }
            }
        } else {
            entry.insert("missing".into(), Value::Bool(true));
        }
        media.push(Value::Object(entry));
    }
    let mut timeline = None;
    let mut timeline_path = None;
    let mut diagnostics = None;
    if let Some(artifacts) = artifacts_dir {
        let manifest: Value = fs::read(artifacts.join("run-manifest.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| json!({}));
        let built = build_timeline(
            &report,
            &summary,
            &media,
            artifacts,
            manifest.get("startedAt").and_then(Value::as_str),
        );
        let path = artifacts.join("timeline.json");
        write_json(&path, &built)?;
        diagnostics = Some(summarize_diagnostics(&report, &built, artifacts));
        timeline_path = Some(path);
        timeline = Some(built);
    }
    let known: BTreeSet<PathBuf> = media
        .iter()
        .filter_map(|item| item.get("file").and_then(Value::as_str).map(PathBuf::from))
        .chain(timeline_path.clone())
        .chain(
            diagnostics
                .as_ref()
                .and_then(|value| value.get("file"))
                .and_then(Value::as_str)
                .map(PathBuf::from),
        )
        .collect();
    let inventory: Vec<Value> = artifacts_dir
        .map(|artifacts| {
            walk(artifacts, false)
                .into_iter()
                .filter(|file| file != report_path && !known.contains(file))
                .map(|file| json!({ "file": file, "sizeKb": size_kb(&file) }))
                .collect()
        })
        .unwrap_or_default();
    let mut output = summary.as_object().expect("object").clone();
    output.insert(
        "runId".into(),
        report_run
            .map(|value| Value::String(value.into()))
            .unwrap_or(Value::Null),
    );
    output.insert("reportPath".into(), json!(report_path));
    output.insert(
        "artifactsDir".into(),
        artifacts_dir.map(|path| json!(path)).unwrap_or(Value::Null),
    );
    output.insert(
        "captureErrors".into(),
        report
            .pointer("/probierz/captureErrors")
            .filter(|value| value.is_array())
            .cloned()
            .unwrap_or_else(|| json!([])),
    );
    output.insert("media".into(), Value::Array(media));
    output.insert("artifacts".into(), Value::Array(inventory));
    output.insert("timeline".into(), timeline.as_ref().map(|timeline| json!({ "path": timeline_path, "counts": timeline["counts"], "diagnostics": timeline["diagnostics"] })).unwrap_or(Value::Null));
    output.insert("diagnostics".into(), diagnostics.unwrap_or(Value::Null));
    Ok(Value::Object(output))
}

pub fn analyze(_harness: &Path, report: &str, args: &[String]) -> Answer {
    let opts = parse_run_args(args, true)?;
    let artifacts = args
        .first()
        .filter(|arg| !arg.starts_with("--"))
        .map(PathBuf::from);
    let result = analyze_run(
        Path::new(report),
        artifacts.as_deref(),
        opts.tool.as_deref(),
        opts.frames,
        None,
    )?;
    print_json(&result)
}

fn sensitive_key(name: &str) -> bool {
    [
        "auth",
        "cookie",
        "credential",
        "email",
        "gmail",
        "key",
        "otp",
        "password",
        "pii",
        "secret",
        "session",
        "token",
    ]
    .iter()
    .any(|part| name.to_ascii_lowercase().contains(part))
}
fn segment(value: Option<&str>, fallback: &str) -> String {
    let source = value.unwrap_or(fallback).trim();
    let mut result = String::new();
    let mut dash = false;
    for ch in source.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            result.push(ch);
            dash = false;
        } else if !dash {
            result.push('-');
            dash = true;
        }
    }
    if result.is_empty() {
        fallback.into()
    } else {
        result
    }
}
fn unique_run_id(started: DateTime<Utc>) -> String {
    let seed = format!(
        "{}:{}:{}",
        started.timestamp_nanos_opt().unwrap_or_default(),
        std::process::id(),
        std::thread::current().name().unwrap_or("main")
    );
    let digest = Sha256::digest(seed.as_bytes());
    let uuid = format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        u32::from_be_bytes(digest[0..4].try_into().expect("slice")),
        u16::from_be_bytes(digest[4..6].try_into().expect("slice")),
        u16::from_be_bytes(digest[6..8].try_into().expect("slice")) & 0x0fff,
        (u16::from_be_bytes(digest[8..10].try_into().expect("slice")) & 0x3fff) | 0x8000,
        u64::from_be_bytes(digest[10..18].try_into().expect("slice")) & 0x0000ffffffffffff
    );
    format!(
        "{}-{uuid}",
        started
            .to_rfc3339_opts(SecondsFormat::Millis, true)
            .replace([':', '.'], "-")
    )
}
fn sha256_file(file: &Path) -> Result<String, Failure> {
    let mut source = File::open(file)?;
    let mut hash = Sha256::new();
    std::io::copy(&mut source, &mut hash).map_err(Failure::from)?;
    Ok(hex::encode(hash.finalize()))
}

#[cfg(unix)]
fn file_mode(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.mode() & 0o777
}
#[cfg(not(unix))]
fn file_mode(_meta: &fs::Metadata) -> u32 {
    0
}

fn git_source_paths(
    root: &Path,
    exclude_secrets: bool,
    package_lock: bool,
) -> Result<Vec<String>, Failure> {
    let result = capture(
        "git",
        &[
            "-C".into(),
            root.to_string_lossy().into_owned(),
            "ls-files".into(),
            "--cached".into(),
            "--others".into(),
            "--exclude-standard".into(),
            "-z".into(),
        ],
        None,
        None,
        None,
    );
    if !result.status.is_some_and(|status| status.success()) {
        return Err(Failure::config(
            "run.source",
            format!(
                "git ls-files in {}: {}",
                root.display(),
                text(&result.stderr).trim()
            ),
        ));
    }
    let mut paths: BTreeSet<String> = text(&result.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .filter(|relative| {
            let parts: Vec<&str> = relative.split('/').collect();
            !Path::new(relative).is_absolute()
                && !parts.contains(&"..")
                && !parts
                    .iter()
                    .any(|part| matches!(*part, "node_modules" | "test-results"))
                && (!exclude_secrets
                    || (!parts.iter().any(|part| part.starts_with(".env"))
                        && !(relative
                            .rsplit('/')
                            .next()
                            .unwrap_or("")
                            .starts_with("probierz-")
                            && relative.ends_with(".json"))))
        })
        .filter(|relative| {
            fs::symlink_metadata(root.join(relative))
                .is_ok_and(|meta| meta.is_file() || meta.file_type().is_symlink())
        })
        .map(str::to_string)
        .collect();
    if package_lock && root.join("package-lock.json").exists() {
        paths.insert("package-lock.json".into());
    }
    Ok(paths.into_iter().collect())
}

fn repository_identity(
    root: &Path,
    name: &str,
    index: Option<usize>,
    exclude_secrets: bool,
    package_lock: bool,
) -> Result<Value, Failure> {
    let files = git_source_paths(root, exclude_secrets, package_lock)?;
    let mut hash = Sha256::new();
    for relative in files {
        let file = root.join(&relative);
        let meta = fs::symlink_metadata(&file)?;
        let (kind, payload) = if meta.file_type().is_symlink() {
            (
                "symlink",
                fs::read_link(&file)?.to_string_lossy().as_bytes().to_vec(),
            )
        } else {
            ("file", fs::read(&file)?)
        };
        let header = json!({ "path": relative, "kind": kind, "mode": file_mode(&meta), "bytes": payload.len() }).to_string();
        hash.update(format!("{}:", header.len()).as_bytes());
        hash.update(header.as_bytes());
        hash.update(payload);
    }
    let worktree = hex::encode(hash.finalize());
    let head = capture(
        "git",
        &[
            "-C".into(),
            root.to_string_lossy().into_owned(),
            "rev-parse".into(),
            "HEAD".into(),
        ],
        None,
        None,
        None,
    );
    let diff = capture(
        "git",
        &[
            "-C".into(),
            root.to_string_lossy().into_owned(),
            "diff".into(),
            "--quiet".into(),
            "HEAD".into(),
            "--".into(),
        ],
        None,
        None,
        None,
    );
    let others = capture(
        "git",
        &[
            "-C".into(),
            root.to_string_lossy().into_owned(),
            "ls-files".into(),
            "--others".into(),
            "--exclude-standard".into(),
            "-z".into(),
        ],
        None,
        None,
        None,
    );
    let mut exact = Map::new();
    if let Some(index) = index {
        exact.insert("index".into(), json!(index));
    }
    exact.insert("name".into(), json!(name));
    exact.insert("worktreeSha256".into(), json!(worktree));
    let sha = hex::encode(Sha256::digest(
        Value::Object(exact.clone()).to_string().as_bytes(),
    ));
    let mut result = Map::new();
    if let Some(index) = index {
        result.insert("index".into(), json!(index));
    }
    result.insert("name".into(), json!(name));
    result.insert(
        "gitSha".into(),
        if head.status.is_some_and(|status| status.success()) {
            json!(text(&head.stdout).trim())
        } else {
            Value::Null
        },
    );
    result.insert(
        "dirty".into(),
        json!(!diff.status.is_some_and(|status| status.success()) || !others.stdout.is_empty()),
    );
    result.insert("worktreeSha256".into(), json!(worktree));
    result.insert("sha256".into(), json!(sha));
    Ok(Value::Object(result))
}

fn submitted_source_identity(app_id: Option<&str>) -> Result<Option<Value>, Failure> {
    let Some(file) = std::env::var_os("PROBIERZ_SOURCE_IDENTITY").map(PathBuf::from) else {
        return Ok(None);
    };
    let document: Value = serde_json::from_slice(&fs::read(&file)?)
        .map_err(|error| Failure::config("run.source", format!("{}: {error}", file.display())))?;
    let valid_hash = |value: Option<&str>| {
        value.is_some_and(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
    };
    if document.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || !valid_hash(
            document
                .pointer("/harness/worktreeSha256")
                .and_then(Value::as_str),
        )
    {
        return Err(Failure::config(
            "run.source",
            format!("{}: unusable source identity", file.display()),
        ));
    }
    for repository in document
        .pointer("/app/repositories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !valid_hash(repository.get("worktreeSha256").and_then(Value::as_str)) {
            return Err(Failure::config(
                "run.source",
                format!("{}: repository has no worktreeSha256", file.display()),
            ));
        }
    }
    if app_id.is_some()
        && document.get("appId").and_then(Value::as_str).is_some()
        && document.get("appId").and_then(Value::as_str) != app_id
    {
        return Ok(None);
    }
    Ok(Some(document))
}

pub fn app_source_identity(harness: &Path, app_id: &str) -> Result<Value, Failure> {
    if let Some(submitted) = submitted_source_identity(Some(app_id))? {
        return Ok(submitted);
    }
    let declaration = manifest::load(harness, app_id)?;
    let repositories = declaration
        .document
        .get("repositories")
        .and_then(serde_yaml::Value::as_sequence)
        .cloned()
        .unwrap_or_default();
    let mut identities = Vec::new();
    for (index, repository) in repositories.iter().enumerate() {
        let root = repository
            .get("root")
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or("");
        identities.push(repository_identity(
            Path::new(root),
            Path::new(root)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
            Some(index),
            false,
            false,
        )?);
    }
    let exact: Vec<Value> = identities
        .iter()
        .map(|identity| json!({ "index": identity["index"], "sha256": identity["sha256"] }))
        .collect();
    let app = json!({ "sha256": hex::encode(Sha256::digest(Value::Array(exact).to_string().as_bytes())), "repositories": identities });
    Ok(
        json!({ "schemaVersion": 1, "appId": app_id, "harness": repository_identity(harness, "probierz", None, true, true)?, "app": app }),
    )
}

fn build_identity(harness: &Path, env: &BTreeMap<String, String>) -> Result<Value, Failure> {
    let candidate = [
        "PROBIERZ_BUILD_PATH",
        "APP_IOS",
        "MAC_APP_PATH",
        "ELECTRON_APP_MAIN",
    ]
    .iter()
    .find_map(|name| env.get(*name).cloned())
    .unwrap_or_else(|| {
        harness
            .join("package-lock.json")
            .to_string_lossy()
            .into_owned()
    });
    let candidate_path = Path::new(&candidate);
    let resolved = if candidate_path.is_absolute() {
        candidate_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(candidate_path)
    };
    let path = normalize_path(&resolved);
    let sha = if path.is_file() {
        Some(sha256_file(&path)?)
    } else if path.is_dir() {
        let mut hash = Sha256::new();
        for file in walk(&path, true) {
            hash.update(
                file.strip_prefix(&path)
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .as_bytes(),
            );
            hash.update(fs::read(file)?);
        }
        Some(hex::encode(hash.finalize()))
    } else {
        None
    };
    Ok(json!({ "path": path, "sha256": sha }))
}

fn write_json(file: &Path, value: &Value) -> Result<(), Failure> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = file.with_file_name(format!(
        "{}.tmp",
        file.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("value")
    ));
    let mut output = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode_600()
        .open(&temporary)?;
    writeln!(output, "{}", serde_json::to_string_pretty(value)?)?;
    fs::rename(temporary, file)?;
    Ok(())
}
trait Mode600 {
    fn mode_600(&mut self) -> &mut Self;
}
impl Mode600 for OpenOptions {
    fn mode_600(&mut self) -> &mut Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            self.mode(0o600);
        }
        self
    }
}

fn update_json(file: &Path, patch: &Value) -> Result<(), Failure> {
    let mut current: Value = fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| json!({}));
    if let (Some(target), Some(values)) = (current.as_object_mut(), patch.as_object()) {
        target.extend(values.clone());
    }
    write_json(file, &current)
}
fn artifact_hashes(directory: &Path, manifest_path: &Path) -> Result<Value, Failure> {
    let mut values = Vec::new();
    for file in walk(directory, true)
        .into_iter()
        .filter(|file| file != manifest_path)
    {
        values.push(json!({ "file": slash(file.strip_prefix(directory).unwrap_or(&file)), "sha256": sha256_file(&file)?, "bytes": fs::metadata(file)?.len() }));
    }
    Ok(Value::Array(values))
}
fn redacted_environment(values: &BTreeMap<String, String>) -> Value {
    Value::Object(
        values
            .iter()
            .map(|(name, value)| {
                let public = if sensitive_key(name) {
                    format!("[REDACTED:{name}]")
                } else {
                    value.clone()
                };
                (name.clone(), Value::String(public))
            })
            .collect(),
    )
}
fn run_conditions(record: bool, values: &BTreeMap<String, String>) -> Value {
    let mut conditions = Map::new();
    conditions.insert("record".into(), Value::Bool(record));
    for (name, value) in values {
        let public = if sensitive_key(name) {
            format!("[REDACTED:{name}]")
        } else {
            value.clone()
        };
        conditions.insert(name.clone(), Value::String(public));
    }
    Value::Object(conditions)
}
fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        architecture => architecture,
    }
}
fn secret_values(values: &BTreeMap<String, String>) -> Vec<(String, String)> {
    values
        .iter()
        .filter(|(name, value)| sensitive_key(name) && value.len() >= 4)
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}
fn redact_text(value: &str, secrets: &[(String, String)]) -> String {
    let mut safe = value.to_string();
    for (name, secret) in secrets {
        safe = safe.replace(secret, &format!("[REDACTED:{name}]"));
    }
    let expression = Regex::new(r"(?i)((?:AUTH|COOKIE|CREDENTIAL|EMAIL|GMAIL|KEY|OTP|PASSWORD|SECRET|SESSION|TOKEN)[A-Z0-9_]*\s*[=:]\s*)[^\s,;]+").expect("regex");
    safe = expression.replace_all(&safe, "$1[REDACTED]").into_owned();
    Regex::new(r#"(?i)("(?:auth|cookie|credential|email|gmail|key|otp|password|secret|session|token)[^"]*"\s*:\s*")[^"]*""#).expect("regex").replace_all(&safe, "$1[REDACTED]\"").into_owned()
}
fn stamped(value: &str) -> String {
    let stamp = now_iso();
    value
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{stamp} {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn tail_chars(value: &str, count: usize) -> String {
    let length = value.chars().count();
    value.chars().skip(length.saturating_sub(count)).collect()
}

fn app_surface(
    harness: &Path,
    app_id: &str,
    target: &str,
) -> Result<(manifest::Manifest, serde_yaml::Value), Failure> {
    let declaration = manifest::load(harness, app_id)?;
    let surface = declaration
        .document
        .get("surfaces")
        .and_then(|surfaces| surfaces.get(target))
        .cloned()
        .ok_or_else(|| {
            Failure::config("run.app", format!("app {app_id} has no {target} surface"))
        })?;
    Ok((declaration, surface))
}
fn yaml_map_strings(value: Option<&serde_yaml::Value>) -> BTreeMap<String, String> {
    value
        .and_then(serde_yaml::Value::as_mapping)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| Some((key.as_str()?.to_string(), yaml_string(value)?)))
                .collect()
        })
        .unwrap_or_default()
}
fn yaml_ordered_strings(value: Option<&serde_yaml::Value>) -> Map<String, Value> {
    let mut result = Map::new();
    if let Some(values) = value.and_then(serde_yaml::Value::as_mapping) {
        for (name, value) in values {
            if let (Some(name), Some(value)) = (name.as_str(), yaml_string(value)) {
                result.insert(name.into(), Value::String(value));
            }
        }
    }
    result
}

fn run_data_command(
    harness: &Path,
    config: Option<&serde_yaml::Value>,
    env: &BTreeMap<String, String>,
    secrets: &[(String, String)],
    stdout: &Path,
    stderr: &Path,
) -> Value {
    let Some(config) = config else {
        return json!({ "ok": true, "result": null });
    };
    let args = match config.get("args") {
        Some(value) => match value.as_sequence() {
            Some(values) => values
                .iter()
                .map(|value| yaml_string(value).unwrap_or_default())
                .collect::<Vec<_>>(),
            None => return json!({ "ok": false, "error": "invalid data command configuration" }),
        },
        None => Vec::new(),
    };
    if let Some(capability) = config.get("capability").and_then(serde_yaml::Value::as_str) {
        return match crate::apphooks::execute(harness, capability, &args, env) {
            Ok(result) => json!({ "ok": true, "result": result }),
            Err(error) => json!({ "ok": false, "error": error.detail }),
        };
    }
    let Some(command) = config.get("command").and_then(serde_yaml::Value::as_str) else {
        return json!({ "ok": false, "error": "invalid data command configuration" });
    };
    let cwd = config
        .get("cwd")
        .and_then(serde_yaml::Value::as_str)
        .map(|cwd| normalize_path(&harness.join(cwd)))
        .unwrap_or_else(|| harness.to_path_buf());
    let timeout = config
        .get("timeoutMs")
        .and_then(serde_yaml::Value::as_u64)
        .filter(|value| *value > 0)
        .unwrap_or(120_000);
    let execution = capture(command, &args, Some(&cwd), Some(env), Some(timeout));
    let safe_out = redact_text(&text(&execution.stdout), secrets);
    let safe_err = redact_text(&text(&execution.stderr), secrets);
    if !safe_out.is_empty() {
        let _ = append_secure(stdout, stamped(&safe_out).as_bytes());
    }
    if !safe_err.is_empty() {
        let _ = append_secure(stderr, stamped(&safe_err).as_bytes());
    }
    if execution.error.is_some() || !execution.status.is_some_and(|status| status.success()) {
        let detail = execution.error.unwrap_or_else(|| {
            let trimmed = safe_err.trim();
            if trimmed.is_empty() {
                format!(
                    "exit {}",
                    execution
                        .status
                        .and_then(|status| status.code())
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "null".into())
                )
            } else {
                trimmed.into()
            }
        });
        return json!({ "ok": false, "error": tail_chars(&detail, TAIL), "exitCode": execution.status.and_then(|status| status.code()) });
    }
    let stdout_text = text(&execution.stdout);
    let lines: Vec<&str> = stdout_text
        .lines()
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return json!({ "ok": true, "result": null });
    }
    match serde_json::from_str::<Value>(lines.last().expect("not empty")) {
        Ok(result) => json!({ "ok": true, "result": result }),
        Err(_) => json!({ "ok": false, "error": "data command did not return JSON" }),
    }
}
fn append_secure(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).append(true).mode_600();
    options.open(path)?.write_all(bytes)
}

fn report_identity(path: &Path, run_id: &str, started: SystemTime) -> Value {
    if !path.exists() {
        return json!({ "ok": false, "error": "report missing" });
    }
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) => return json!({ "ok": false, "error": format!("report unreadable: {error}") }),
    };
    if meta
        .modified()
        .ok()
        .is_some_and(|modified| modified < started)
    {
        return json!({ "ok": false, "error": "report predates run start" });
    }
    match fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    {
        Some(report) => {
            let actual = report.pointer("/probierz/runId").and_then(Value::as_str);
            if actual != Some(run_id) {
                json!({ "ok": false, "error": format!("report run ID mismatch: expected {run_id}, got {}", actual.unwrap_or("missing")) })
            } else {
                json!({ "ok": true, "runId": run_id, "mtime": meta.modified().map(system_time_iso).unwrap_or_else(|_| now_iso()) })
            }
        }
        None => json!({ "ok": false, "error": "report unreadable: invalid JSON" }),
    }
}

fn redact_diagnostic(value: &str) -> String {
    Regex::new(r"([?&][^=\s&]+)=([^&\s]+)")
        .expect("regex")
        .replace_all(&safe_message(value), "$1=[VALUE]")
        .into_owned()
}

fn simulator_identifier(requested: Option<&str>) -> String {
    let requested = requested.unwrap_or("booted");
    if requested == "booted" {
        return requested.into();
    }
    let document = simctl_devices();
    let candidates: Vec<&Value> = document
        .get("devices")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|devices| devices.values())
        .filter_map(Value::as_array)
        .flatten()
        .filter(|device| {
            device.get("udid").and_then(Value::as_str) == Some(requested)
                || device.get("name").and_then(Value::as_str) == Some(requested)
        })
        .collect();
    candidates
        .iter()
        .find(|device| device.get("state").and_then(Value::as_str) == Some("Booted"))
        .copied()
        .or_else(|| candidates.first().copied())
        .and_then(|device| device.get("udid").and_then(Value::as_str))
        .unwrap_or(requested)
        .to_string()
}

fn write_secure_text(path: &Path, value: &str) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true).mode_600();
    options.open(path)?.write_all(value.as_bytes())
}

fn collect_platform_diagnostics(
    target: &str,
    env: &BTreeMap<String, String>,
    artifacts: &Path,
    started: &str,
) -> Value {
    let directory = artifacts.join("diagnostics");
    let _ = fs::create_dir_all(&directory);
    let app_path = if matches!(target, "desktop:mac" | "desktop:cua") {
        env.get("MAC_APP_PATH")
    } else {
        env.get("APP_IOS")
    };
    let process_name = app_path
        .and_then(|path| Path::new(path).file_stem())
        .and_then(|name| name.to_str())
        .map(str::to_string);
    let elapsed = DateTime::parse_from_rfc3339(started)
        .map(|date| {
            ((Utc::now().timestamp_millis() - date.timestamp_millis()) as f64 / 1000.0).ceil()
                as i64
                + 5
        })
        .unwrap_or(60)
        .max(1);
    let (command, args, file) =
        if matches!(target, "desktop:mac" | "desktop:cua") && process_name.is_some() {
            let name = process_name.expect("checked");
            (
                "/usr/bin/log",
                vec![
                    "show".into(),
                    "--style".into(),
                    "compact".into(),
                    "--last".into(),
                    format!("{elapsed}s"),
                    "--predicate".into(),
                    format!("process == \"{name}\""),
                ],
                directory.join("macos-unified.log"),
            )
        } else if target == "mobile:ios" && process_name.is_some() {
            let name = process_name.expect("checked");
            (
                "xcrun",
                vec![
                    "simctl".into(),
                    "spawn".into(),
                    simulator_identifier(env.get("IOS_DEVICE").map(String::as_str)),
                    "log".into(),
                    "show".into(),
                    "--style".into(),
                    "compact".into(),
                    "--last".into(),
                    format!("{elapsed}s"),
                    "--predicate".into(),
                    format!("process == \"{name}\""),
                ],
                directory.join("ios-simulator.log"),
            )
        } else if target == "mobile:android" {
            let mut args = Vec::new();
            if let Some(device) = env.get("ANDROID_DEVICE") {
                args.extend(["-s".into(), device.clone()]);
            }
            args.extend([
                "logcat".into(),
                "-d".into(),
                "-v".into(),
                "threadtime".into(),
            ]);
            ("adb", args, directory.join("android-logcat.log"))
        } else {
            return json!({ "supported": false, "file": null });
        };
    let result = capture(command, &args, None, None, Some(20_000));
    let output = redact_diagnostic(&text(&result.stdout));
    let error = redact_diagnostic(&text(&result.stderr));
    let _ = write_secure_text(&file, &output);
    let ok = result.status.is_some_and(|status| status.success());
    let failure = if ok {
        Value::Null
    } else {
        let detail = if error.trim().is_empty() {
            result.error.as_deref().unwrap_or("collector failed")
        } else {
            error.trim()
        };
        Value::String(tail_chars(detail, 2000))
    };
    json!({
        "supported": true,
        "file": file,
        "ok": ok,
        "exitCode": result.status.and_then(|status| status.code()).unwrap_or(-1),
        "error": failure,
    })
}

fn named_process_sample(process_name: Option<&str>) -> Option<Value> {
    let process_name = process_name?;
    if cfg!(windows) {
        return None;
    }
    let result = capture_text("ps", &["-axo", "comm=,rss=,%cpu="], None, Some(3000));
    if !result.status.is_some_and(|status| status.success()) {
        return None;
    }
    let expression = Regex::new(r"^(.*?)\s+(\d+)\s+([\d.]+)$").expect("regex");
    let mut rss_kb = 0.0;
    let mut cpu_percent = 0.0;
    let mut processes = 0;
    for line in text(&result.stdout).lines() {
        let Some(parts) = expression.captures(line.trim()) else {
            continue;
        };
        if Path::new(&parts[1])
            .file_name()
            .and_then(|name| name.to_str())
            != Some(process_name)
        {
            continue;
        }
        rss_kb += parts[2].parse::<f64>().unwrap_or(0.0);
        cpu_percent += parts[3].parse::<f64>().unwrap_or(0.0);
        processes += 1;
    }
    (processes > 0).then(|| {
        json!({
            "processes": processes,
            "rssKb": number(rss_kb),
            "cpuPercent": number(cpu_percent),
        })
    })
}

fn performance_sample(pgid: u32, process_name: Option<&str>) -> Option<Value> {
    if cfg!(windows) {
        return None;
    }
    let result = capture_text("ps", &["-axo", "pgid=,rss=,%cpu="], None, Some(3000));
    if !result.status.is_some_and(|status| status.success()) {
        return None;
    }
    let mut rss_kb = 0.0;
    let mut cpu_percent = 0.0;
    let mut processes = 0;
    for line in text(&result.stdout).lines() {
        let values: Vec<&str> = line.split_whitespace().collect();
        if values.first().and_then(|value| value.parse::<u32>().ok()) != Some(pgid) {
            continue;
        }
        rss_kb += values
            .get(1)
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0);
        cpu_percent += values
            .get(2)
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0);
        processes += 1;
    }
    Some(json!({
        "at": now_iso(),
        "processes": processes,
        "rssKb": number(rss_kb),
        "cpuPercent": number(cpu_percent),
        "app": named_process_sample(process_name),
    }))
}

struct RunOptions {
    env: BTreeMap<String, String>,
    /// Run the byk-auth suite on this machine instead of the dedicated host.
    local: bool,
    /// The fleet host the remote byk-auth suite is placed on.
    host_selector: String,
    record: bool,
    timeout_ms: u64,
    force: bool,
    spec: Option<String>,
    app_id: Option<String>,
    kind: Option<String>,
    resource_wait_ms: Option<u64>,
}

fn drain_run_stream<R: Read>(
    mut stream: R,
    path: &Path,
    secrets: &[(String, String)],
    run_started: DateTime<Utc>,
) -> (String, Option<u64>) {
    let mut tail = String::new();
    let mut first_output_ms = None;
    let mut buffer = [0u8; 8192];
    loop {
        let count = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        if first_output_ms.is_none() {
            first_output_ms = Some(
                (Utc::now().timestamp_millis() - run_started.timestamp_millis()).max(0) as u64,
            );
        }
        let safe = redact_text(&String::from_utf8_lossy(&buffer[..count]), secrets);
        tail = tail_chars(&(tail + &safe), TAIL);
        let _ = append_secure(path, stamped(&safe).as_bytes());
    }
    (tail, first_output_ms)
}

struct BykBroker {
    child: Child,
    directory: PathBuf,
    socket_path: PathBuf,
    recipient: String,
}

impl Drop for BykBroker {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = Command::new("/bin/kill")
                .args(["-TERM", &self.child.id().to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let until = Instant::now() + Duration::from_millis(2000);
            while Instant::now() < until && self.child.try_wait().ok().flatten().is_none() {
                thread::sleep(Duration::from_millis(20));
            }
            if self.child.try_wait().ok().flatten().is_none() {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn byk_broker_environment(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut answer = env.clone();
    if !answer.contains_key("SKARBIEC_UNLOCK") {
        let file = answer
            .get("SKARBIEC_UNLOCK_FILE")
            .map(PathBuf::from)
            .or_else(|| {
                answer
                    .get("HOME")
                    .map(|home| PathBuf::from(home).join(".skarbiec-unlock"))
            });
        if let Some(file) = file {
            if let Ok(value) = fs::read_to_string(file) {
                let value = value.trim();
                if !value.is_empty() {
                    answer.insert("SKARBIEC_UNLOCK".into(), value.into());
                }
            }
        }
    }
    answer
}

fn byk_startup_error(message: &str, stderr: &Arc<Mutex<Vec<u8>>>) -> String {
    let safe = stderr
        .lock()
        .ok()
        .map(|raw| {
            let text = String::from_utf8_lossy(&raw);
            tail_chars(&text, TAIL)
                .lines()
                .filter(|line| !line.is_empty())
                .map(|_| "[REDACTED]")
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if safe.is_empty() {
        message.to_string()
    } else {
        format!("{message}\nbroker stderr (sanitized, truncated):\n{safe}")
    }
}

fn valid_byk_recipient(value: &str) -> bool {
    value.len() <= 254
        && Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$")
            .expect("recipient regex")
            .is_match(value)
}

/// The login mailbox this target reads its one-time codes from, and the file
/// that holds the address a resend is sent from. Both are the target's, not a
/// caller's choice: a journey that authenticates a real account has exactly
/// one mailbox.
const BYK_MAILBOX: &str = "byk-ios-login";

/// Where the mailbox broker executable comes from.
///
/// A harness does not build another repository. This used to `cargo build
/// --bin skarbiec-entitlements-router` inside `entitlements-rotator`, which
/// stopped existing on 2026-07-28 when that repository removed its vendored
/// copy of the vault (commit 525f7d6, "Stop being a second source and
/// publisher of Skarbiec"). The journey kept building a binary nobody
/// produced any more and reported it as a build failure, which hid what had
/// actually happened.
///
/// So the broker is now what it always was in truth: an operator-provisioned
/// executable. `BYK_MAILBOX_BROKER` names it, and the refusal says what it
/// must be able to do.
fn byk_broker_binary(
    _harness: &Path,
    env: &BTreeMap<String, String>,
    _timeout_ms: u64,
) -> Result<(PathBuf, PathBuf, BTreeMap<String, String>), String> {
    // The operator's shell counts: a `KEY=VALUE` argument wins, and an
    // exported variable is honoured, exactly as every other condition is.
    let broker_env = byk_broker_environment(&env_snapshot(env));
    let declared = broker_env
        .get("BYK_MAILBOX_BROKER")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!(
            "BYK_MAILBOX_BROKER is required: an executable serving `mailbox-broker --mailbox {BYK_MAILBOX} --socket <path>`, \
`mailbox-probe --mailbox {BYK_MAILBOX}` and `seed-resend <env-file>`. \
No installed product provides it: entitlements-rotator removed its vendored vault binary in 525f7d6 on 2026-07-28 and the surviving copy is the vendored-superset branch of wisent-ai/skarbiec"
        ))?;
    let broker = PathBuf::from(&declared);
    if !broker.is_absolute() {
        return Err(format!(
            "BYK_MAILBOX_BROKER must be an absolute path, not {declared}"
        ));
    }
    let metadata = fs::metadata(&broker)
        .map_err(|error| format!("BYK_MAILBOX_BROKER {declared} cannot be read: {error}"))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!(
            "BYK_MAILBOX_BROKER {declared} is not an executable file"
        ));
    }
    let working = broker
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));
    Ok((broker, working, broker_env))
}

/// Is the login mailbox reachable from here? This is the readiness question
/// `check` asks, and it is the only one that cannot be answered by looking at
/// a file: the broker has to open the mailbox and say so.
fn byk_mailbox_reachable(harness: &Path, env: &BTreeMap<String, String>) -> (bool, String) {
    let (broker, rotator, broker_env) = match byk_broker_binary(harness, env, DEFAULT_TIMEOUT_MS) {
        Ok(parts) => parts,
        Err(reason) => return (false, reason),
    };
    let probe = capture(
        broker.to_string_lossy().as_ref(),
        &[
            "mailbox-probe".into(),
            "--mailbox".into(),
            BYK_MAILBOX.into(),
        ],
        Some(&rotator),
        Some(&broker_env),
        Some(DEFAULT_TIMEOUT_MS),
    );
    if probe.status.is_some_and(|status| status.success()) {
        return (true, String::new());
    }
    let detail = tail_chars(&String::from_utf8_lossy(&probe.stderr), 400)
        .trim()
        .to_string();
    (
        false,
        if detail.is_empty() {
            format!("the {BYK_MAILBOX} mailbox did not answer")
        } else {
            format!("the {BYK_MAILBOX} mailbox did not answer: {detail}")
        },
    )
}

/// Seed the mailbox's resend source and stop. The journey needs an address a
/// resend can come from; seeding it is an operator action on real mail state,
/// so it is its own mode and never a side effect of running the journey.
fn seed_byk_resend(harness: &Path, env: &BTreeMap<String, String>) -> Answer {
    let (broker, rotator, broker_env) = byk_broker_binary(harness, env, DEFAULT_TIMEOUT_MS)
        .map_err(|reason| fail("run.byk.seed", reason))?;
    let source = harness
        .parent()
        .unwrap_or(harness)
        .join("weles")
        .join(".env");
    if !source.exists() {
        return Err(fail(
            "run.byk.seed",
            format!("the resend source {} does not exist", source.display()),
        ));
    }
    let status = Command::new(&broker)
        .args(["seed-resend", source.to_string_lossy().as_ref()])
        .current_dir(&rotator)
        .envs(&broker_env)
        .stdin(Stdio::null())
        .status()
        .map_err(|error| {
            fail(
                "run.byk.seed",
                format!("could not start the Skarbiec mailbox broker: {error}"),
            )
        })?;
    if !status.success() {
        return Err(fail(
            "run.byk.seed",
            format!(
                "seeding the {BYK_MAILBOX} resend source failed with exit {}",
                status.code().unwrap_or(-1)
            ),
        ));
    }
    print_json(&json!({
        "target": "mobile:ios:byk-auth",
        "action": "seed-resend",
        "mailbox": BYK_MAILBOX,
        "source": source,
        "seeded": true,
    }))
}

fn start_byk_broker(
    harness: &Path,
    env: &BTreeMap<String, String>,
    secrets: &[(String, String)],
    stdout_path: &Path,
    stderr_path: &Path,
    timeout_ms: u64,
) -> Result<BykBroker, String> {
    // The broker is provisioned, not built: see `byk_broker_binary`.
    let (broker, rotator, broker_env) = byk_broker_binary(harness, env, timeout_ms)?;
    let _ = (secrets, stdout_path, stderr_path);

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("probierz-byk-auth-{}-{stamp}", std::process::id()));
    fs::create_dir(&directory).map_err(|error| error.to_string())?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let socket_path = directory.join("byk-otp.sock");
    let mut command = Command::new(&broker);
    command
        .args([
            "mailbox-broker",
            "--mailbox",
            "byk-ios-login",
            "--socket",
            socket_path.to_string_lossy().as_ref(),
        ])
        .current_dir(&rotator)
        .envs(&broker_env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            let _ = fs::remove_dir_all(&directory);
            return Err("could not start the Skarbiec mailbox broker".into());
        }
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "could not read the Skarbiec mailbox broker".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "could not read the Skarbiec mailbox broker".to_string())?;
    let stderr_tail = Arc::new(Mutex::new(Vec::new()));
    let stderr_capture = Arc::clone(&stderr_tail);
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut buffer = [0_u8; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            if let Ok(mut tail) = stderr_capture.lock() {
                tail.extend_from_slice(&buffer[..count]);
                if tail.len() > TAIL {
                    let remove = tail.len() - TAIL;
                    tail.drain(..remove);
                }
            }
        }
    });
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line).and_then(|count| {
            if count == 0 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "broker stdout closed",
                ))
            } else {
                Ok(line)
            }
        });
        let _ = sender.send(result);
        let _ = std::io::copy(&mut reader, &mut std::io::sink());
    });
    let mut owner = BykBroker {
        child,
        directory,
        socket_path,
        recipient: String::new(),
    };
    let line = match receiver.recv_timeout(Duration::from_millis(15_000)) {
        Ok(Ok(line)) if line.len() <= 16_384 => line.trim_end_matches(['\r', '\n']).to_string(),
        Ok(Ok(_)) => {
            return Err(byk_startup_error(
                "Skarbiec mailbox broker readiness line was too large",
                &stderr_tail,
            ));
        }
        Ok(Err(_)) => {
            return Err(byk_startup_error(
                "Skarbiec mailbox broker exited before readiness",
                &stderr_tail,
            ));
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(byk_startup_error(
                "timed out waiting for the Skarbiec mailbox broker",
                &stderr_tail,
            ));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(byk_startup_error(
                "Skarbiec mailbox broker exited before readiness",
                &stderr_tail,
            ));
        }
    };
    let readiness: Value = serde_json::from_str(&line).map_err(|_| {
        byk_startup_error(
            "Skarbiec mailbox broker returned invalid readiness JSON",
            &stderr_tail,
        )
    })?;
    if !readiness.is_object()
        || readiness.get("status").and_then(Value::as_str) != Some("ready")
        || readiness.get("mailbox").and_then(Value::as_str) != Some("byk-ios-login")
    {
        return Err(byk_startup_error(
            "Skarbiec mailbox broker returned invalid readiness data",
            &stderr_tail,
        ));
    }
    let socket = readiness
        .get("socket_path")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            byk_startup_error(
                "Skarbiec mailbox broker returned an invalid socket path",
                &stderr_tail,
            )
        })?;
    if !Path::new(socket).is_absolute() || Path::new(socket) != owner.socket_path {
        return Err(byk_startup_error(
            "Skarbiec mailbox broker returned an invalid socket path",
            &stderr_tail,
        ));
    }
    let recipient = readiness
        .get("recipient")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            byk_startup_error(
                "Skarbiec mailbox broker returned an invalid recipient",
                &stderr_tail,
            )
        })?;
    if !valid_byk_recipient(recipient) {
        return Err(byk_startup_error(
            "Skarbiec mailbox broker returned an invalid recipient",
            &stderr_tail,
        ));
    }
    if !fs::metadata(&owner.socket_path)
        .map(|value| value.file_type().is_socket())
        .unwrap_or(false)
    {
        return Err(byk_startup_error(
            "Skarbiec mailbox broker did not create a Unix socket",
            &stderr_tail,
        ));
    }
    owner.recipient = recipient.to_string();
    Ok(owner)
}

/// The byk-auth journey: a real Apple ID login whose one-time code arrives in
/// a real mailbox. The broker owns the mailbox end and hands the suite a
/// socket and the address the code was sent to.
///
/// `local` decides where the XCUITest suite runs: on this machine's simulator,
/// or on the dedicated host through the fleet. The mailbox side is identical
/// either way, because there is only one login account.
/// Which fleet host the remote suite is placed on: what the operator asked
/// for, what the run environment declares, or the dedicated Mac otherwise.
fn byk_host_selector(selector: Option<&str>, env: &BTreeMap<String, String>) -> String {
    selector
        .map(str::to_string)
        .or_else(|| env.get("BYK_HOST_SELECTOR").cloned())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "stado:mini".to_string())
}

fn execute_byk(
    local: bool,
    host_selector: &str,
    harness: &Path,
    env: &BTreeMap<String, String>,
    timeout_ms: u64,
    secrets: &[(String, String)],
    stdout_path: &Path,
    stderr_path: &Path,
    started_at: &str,
    artifacts: &Path,
) -> Result<(i32, bool, String, String, Value, Value), Failure> {
    let app = env.get("APP_IOS").map(String::as_str).unwrap_or("");
    let bundle = env.get("BUNDLE_ID").map(String::as_str).unwrap_or("");
    let started = Instant::now();
    let result = (|| -> Result<i32, String> {
        if app != app.trim() || bundle != bundle.trim() {
            return Err("APP_IOS and BUNDLE_ID must not contain surrounding whitespace".into());
        }
        if app.is_empty() == bundle.is_empty() {
            return Err("set exactly one of APP_IOS or BUNDLE_ID".into());
        }
        let broker = start_byk_broker(harness, env, secrets, stdout_path, stderr_path, timeout_ms)?;
        if local {
            // The same suite the `mobile:ios` target runs, told which socket
            // carries the code and which address it was sent to.
            let mut suite_env = env.clone();
            suite_env.insert("PROBIERZ_SPEC".into(), "byk-auth.e2e.ts".into());
            suite_env.insert(
                "BYK_OTP_SOCKET".into(),
                broker.socket_path.display().to_string(),
            );
            suite_env.insert("BYK_TEST_EMAIL".into(), broker.recipient.clone());
            let outcome = execute_suite(
                false,
                host_selector,
                harness,
                "test:mobile:ios",
                &suite_env,
                timeout_ms,
                secrets.to_vec(),
                stdout_path,
                stderr_path,
                "mobile:ios:byk-auth",
                started_at,
                artifacts,
            )
            .map_err(|error| error.detail)?;
            return Ok(outcome.0);
        }
        let outcome = crate::stado::run_remote_byk_auth(crate::stado::RemoteBykRequest {
            root: harness,
            app_path: Path::new(app),
            ios_device: env
                .get("IOS_DEVICE")
                .map(String::as_str)
                .unwrap_or("iPhone 15"),
            ios_version: env.get("IOS_VERSION").map(String::as_str).unwrap_or(""),
            socket_path: &broker.socket_path,
            recipient: &broker.recipient,
            host_selector,
        })
        .map_err(|error| error.detail)?;
        Ok(if outcome.signal.is_some() {
            1
        } else {
            outcome.code.unwrap_or(1)
        })
    })();
    let (exit_code, stderr_tail) = match result {
        Ok(code) => (code, String::new()),
        Err(error) => {
            let safe = redact_text(&format!("byk auth runner: {error}\n"), secrets);
            let _ = append_secure(stderr_path, stamped(&safe).as_bytes());
            (1, safe)
        }
    };
    let performance = json!({
        "schemaVersion": 1,
        "subject": "run-and-app-processes",
        "firstOutputMs": Value::Null,
        "intervalMs": SAMPLE_INTERVAL_MS,
        "peakRssKb": Value::Null,
        "averageCpuPercent": Value::Null,
        "appProcessName": Path::new(app).file_stem().and_then(|name| name.to_str()),
        "appPeakRssKb": Value::Null,
        "appAverageCpuPercent": Value::Null,
        "samples": [],
    });
    let performance_path = artifacts.join("performance.json");
    write_json(&performance_path, &performance)?;
    let mut public = performance;
    public
        .as_object_mut()
        .expect("performance object")
        .insert("file".into(), json!(performance_path));
    public
        .as_object_mut()
        .expect("performance object")
        .shift_remove("samples");
    let diagnostics =
        collect_platform_diagnostics("mobile:ios:byk-auth", env, artifacts, started_at);
    let timed_out = started.elapsed() >= Duration::from_millis(timeout_ms) && exit_code != 0;
    Ok((
        exit_code,
        timed_out,
        String::new(),
        stderr_tail,
        public,
        diagnostics,
    ))
}

fn execute_suite(
    // Only `mobile:ios:byk-auth` reads these: run its suite here rather than on
    // the fleet, and which fleet host to place it on when it is remote.
    local: bool,
    host_selector: &str,
    harness: &Path,
    script: &str,
    env: &BTreeMap<String, String>,
    timeout_ms: u64,
    secrets: Vec<(String, String)>,
    stdout_path: &Path,
    stderr_path: &Path,
    target_name: &str,
    started_at: &str,
    artifacts: &Path,
) -> Result<(i32, bool, String, String, Value, Value), Failure> {
    if target_name == "mobile:ios:byk-auth" {
        return execute_byk(
            local,
            host_selector,
            harness,
            env,
            timeout_ms,
            &secrets,
            stdout_path,
            stderr_path,
            started_at,
            artifacts,
        );
    }
    let mut command = Command::new("npm");
    command
        .args(["run", script])
        .current_dir(harness)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        Failure::config(
            "run.spawn",
            format!("Starting the {target_name} runner failed: {error}"),
        )
    })?;
    let pid = child.id();
    let child_out = child.stdout.take().expect("piped stdout");
    let child_err = child.stderr.take().expect("piped stderr");
    let out_path = stdout_path.to_path_buf();
    let err_path = stderr_path.to_path_buf();
    let out_secrets = secrets.clone();
    let err_secrets = secrets;
    let run_started = DateTime::parse_from_rfc3339(started_at)
        .map(|date| date.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let out_thread =
        thread::spawn(move || drain_run_stream(child_out, &out_path, &out_secrets, run_started));
    let err_thread =
        thread::spawn(move || drain_run_stream(child_err, &err_path, &err_secrets, run_started));

    let started = Instant::now();
    let process_name = {
        let app_path = if matches!(target_name, "desktop:mac" | "desktop:cua") {
            env.get("MAC_APP_PATH")
        } else {
            env.get("APP_IOS")
        };
        app_path
            .and_then(|path| Path::new(path).file_stem())
            .and_then(|name| name.to_str())
            .map(str::to_string)
    };
    let mut samples = Vec::new();
    if let Some(sample) = performance_sample(pid, process_name.as_deref()) {
        samples.push(sample);
    }
    let mut next_sample = Instant::now() + Duration::from_millis(SAMPLE_INTERVAL_MS);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= Duration::from_millis(timeout_ms) {
                    timed_out = true;
                    terminate_tree(&mut child, false);
                    thread::sleep(Duration::from_millis(25));
                    if child.try_wait().ok().flatten().is_none() {
                        terminate_tree(&mut child, true);
                    }
                    break child.wait()?;
                }
                if Instant::now() >= next_sample {
                    if let Some(sample) = performance_sample(pid, process_name.as_deref()) {
                        samples.push(sample);
                    }
                    next_sample = Instant::now() + Duration::from_millis(SAMPLE_INTERVAL_MS);
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(Failure::unavailable("run.wait", error.to_string())),
        }
    };
    if let Some(sample) = performance_sample(pid, process_name.as_deref()) {
        samples.push(sample);
    }
    let (safe_out, first_out) = out_thread.join().unwrap_or_default();
    let (safe_err, first_err) = err_thread.join().unwrap_or_default();
    let first_output_ms = match (first_out, first_err) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    };
    let rss: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.get("rssKb").and_then(Value::as_f64))
        .collect();
    let cpu: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.get("cpuPercent").and_then(Value::as_f64))
        .collect();
    let app_rss: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.pointer("/app/rssKb").and_then(Value::as_f64))
        .collect();
    let app_cpu: Vec<f64> = samples
        .iter()
        .filter_map(|sample| sample.pointer("/app/cpuPercent").and_then(Value::as_f64))
        .collect();
    let average = |values: &[f64]| {
        if values.is_empty() {
            Value::Null
        } else {
            number(values.iter().sum::<f64>() / values.len() as f64)
        }
    };
    let performance = json!({
        "schemaVersion": 1,
        "subject": "run-and-app-processes",
        "firstOutputMs": first_output_ms,
        "intervalMs": SAMPLE_INTERVAL_MS,
        "peakRssKb": rss.iter().copied().max_by(f64::total_cmp).map(number).unwrap_or(Value::Null),
        "averageCpuPercent": average(&cpu),
        "appProcessName": process_name,
        "appPeakRssKb": app_rss.iter().copied().max_by(f64::total_cmp).map(number).unwrap_or(Value::Null),
        "appAverageCpuPercent": average(&app_cpu),
        "samples": samples,
    });
    let performance_path = artifacts.join("performance.json");
    write_json(&performance_path, &performance)?;
    let mut public = performance.clone();
    public
        .as_object_mut()
        .expect("object")
        .insert("file".into(), json!(performance_path));
    public
        .as_object_mut()
        .expect("object")
        .shift_remove("samples");
    let diagnostics = collect_platform_diagnostics(target_name, env, artifacts, started_at);
    Ok((
        status.code().unwrap_or(-1),
        timed_out,
        safe_out,
        safe_err,
        public,
        diagnostics,
    ))
}

fn run_surface(harness: &Path, name: &str, mut opts: RunOptions) -> Result<Value, Failure> {
    let config = target(name).ok_or_else(|| {
        fail(
            "run.target",
            format!(
                "unknown target: {name} (one of {})",
                target_list().join(", ")
            ),
        )
    })?;
    let app_id = segment(
        opts.app_id
            .as_deref()
            .or_else(|| opts.env.get("PROBIERZ_APP_ID").map(String::as_str)),
        "probierz",
    );
    let mut app = None;
    let mut surface = None;
    if app_id != "probierz" {
        let (declaration, value) = app_surface(harness, &app_id, name)?;
        let mut conditions = yaml_map_strings(value.get("conditions"));
        conditions.extend(opts.env.clone());
        opts.env = conditions;
        for secret in declaration
            .document
            .get("secretRefs")
            .and_then(serde_yaml::Value::as_mapping)
            .into_iter()
            .flatten()
            .filter_map(|(key, _)| key.as_str())
        {
            if !opts.env.contains_key(secret) {
                if let Ok(value) = std::env::var(secret) {
                    opts.env.insert(secret.into(), value);
                }
            }
        }
        for (target_name, source_name) in value
            .get("env")
            .and_then(serde_yaml::Value::as_mapping)
            .into_iter()
            .flatten()
            .filter_map(|(key, value)| Some((key.as_str()?, value.as_str()?)))
        {
            if let Some(value) = opts
                .env
                .get(source_name)
                .cloned()
                .or_else(|| std::env::var(source_name).ok())
            {
                opts.env.insert(source_name.into(), value.clone());
                opts.env.insert(target_name.into(), value);
            }
        }
        surface = Some(value);
        app = Some(declaration);
    }
    let configured_spec = opts.spec.clone().or_else(|| {
        surface
            .as_ref()
            .and_then(|value| value.get("spec"))
            .and_then(serde_yaml::Value::as_str)
            .map(str::to_string)
    });
    let byk = name == "mobile:ios:byk-auth";
    let record = if byk { false } else { opts.record };
    if byk {
        let allowed = [
            "APP_IOS",
            "BUNDLE_ID",
            "IOS_DEVICE",
            "IOS_VERSION",
            "APPIUM_HOME",
            "DEVELOPER_DIR",
        ];
        if opts
            .env
            .keys()
            .any(|name| !allowed.contains(&name.as_str()))
        {
            return Err(fail("run.conditions","mobile:ios:byk-auth accepts only app, device, runtime, Appium, and Xcode path conditions"));
        }
    }
    let started_date = Utc::now();
    let started_time = SystemTime::now();
    let started_at = started_date.to_rfc3339_opts(SecondsFormat::Millis, true);
    let run_id = unique_run_id(started_date);
    let artifacts = harness
        .join("test-results")
        .join(&app_id)
        .join(segment(Some(name), "target"))
        .join(&started_at[..10])
        .join(&run_id);
    for child in ["media", "frames", "diagnostics"] {
        fs::create_dir_all(artifacts.join(child))?;
    }
    let report_path = artifacts.join("report.json");
    let manifest_path = artifacts.join("run-manifest.json");
    let stdout_path = artifacts.join("stdout.log");
    let stderr_path = artifacts.join("stderr.log");
    let build = build_identity(harness, &opts.env)?;
    let kind = segment(
        opts.kind
            .as_deref()
            .or_else(|| opts.env.get("PROBIERZ_RUN_KIND").map(String::as_str)),
        "adhoc",
    );
    let journeys = surface
        .as_ref()
        .map(|value| manifest::surface_journeys(value, &opts.env))
        .unwrap_or_default();
    let submitted = submitted_source_identity(Some(&app_id))?;
    let (source, harness_identity, origin) = if let Some(submitted) = submitted {
        (
            submitted.pointer("/app").cloned().unwrap_or(Value::Null),
            submitted
                .pointer("/harness")
                .cloned()
                .unwrap_or(Value::Null),
            "submitter",
        )
    } else {
        let source = if app.is_some() {
            app_source_identity(harness, &app_id)?
                .get("app")
                .cloned()
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        (
            source,
            repository_identity(harness, "probierz", None, true, true)?,
            "runner",
        )
    };
    let conditions = run_conditions(record, &opts.env);
    let mut base = json!({"runId":run_id,"startedAt":started_at,"appId":app_id,"kind":kind,"target":name,"tool":config.tool,"pkg":config.pkg,"script":config.script,"artifactsDir":artifacts,"reportPath":report_path,"manifestPath":manifest_path,"stdoutPath":stdout_path,"stderrPath":stderr_path,"conditions":conditions});
    let app_manifest=app.as_ref().map(|declaration|json!({"file":declaration.file,"owner":declaration.document.get("owner").and_then(serde_yaml::Value::as_str).unwrap_or(""),"journeys":journeys})).unwrap_or(Value::Null);
    let host_name = capture_text("hostname", &[], None, Some(3000));
    let release = capture_text("uname", &["-r"], None, Some(3000));
    let node = capture_text("node", &["--version"], None, Some(3000));
    write_json(
        &manifest_path,
        &json!({"schemaVersion":2,"runId":run_id,"appId":app_id,"kind":kind,"target":name,"spec":if byk{json!("byk-auth.e2e.ts")}else{configured_spec.clone().map(Value::String).unwrap_or(Value::Null)},"status":"preflight","startedAt":started_at,"harness":harness_identity,"source":source,"sourceIdentityOrigin":origin,"build":build,"appVersion":opts.env.get("PROBIERZ_APP_VERSION"),"appManifest":app_manifest,"host":{"hostname":text(&host_name.stdout).trim(),"platform":match std::env::consts::OS{"macos"=>"darwin","windows"=>"win32",other=>other},"release":text(&release.stdout).trim(),"arch":node_arch(),"node":text(&node.stdout).trim()},"device":{"name":opts.env.get("IOS_DEVICE").or_else(||opts.env.get("ANDROID_DEVICE")),"runtime":opts.env.get("IOS_VERSION").or_else(||opts.env.get("ANDROID_VERSION"))},"conditions":conditions,"paths":{"artifactsDir":artifacts,"reportPath":report_path,"stdoutPath":stdout_path,"stderrPath":stderr_path}}),
    )?;
    if !opts.force {
        let pf = preflight(harness, if byk { "mobile:ios" } else { name }, &opts.env)?;
        if !pf.get("ready").and_then(Value::as_bool).unwrap_or(false) {
            update_json(
                &manifest_path,
                &json!({"status":"blocked","completedAt":now_iso(),"preflight":pf,"artifacts":artifact_hashes(&artifacts,&manifest_path)?}),
            )?;
            let mut result = base.as_object().expect("object").clone();
            result.extend(
                json!({"ready":false,"skipped":true,"preflight":pf})
                    .as_object()
                    .expect("object")
                    .clone(),
            );
            return Ok(Value::Object(result));
        }
    }
    let mut env = env_snapshot(&opts.env);
    env.insert("PROBIERZ_APP_ID".into(), app_id.clone());
    env.insert("PROBIERZ_RUN_ID".into(), run_id.clone());
    env.insert(
        "PROBIERZ_TOOLKIT_ROOT".into(),
        harness.to_string_lossy().into_owned(),
    );
    env.insert(
        "PROBIERZ_ARTIFACTS".into(),
        artifacts.to_string_lossy().into_owned(),
    );
    env.insert(
        "PROBIERZ_REPORT_PATH".into(),
        report_path.to_string_lossy().into_owned(),
    );
    env.insert("PROBIERZ_JOURNEYS".into(), journeys.join(","));
    env.insert(
        "PROBIERZ_NATIVE_CAPTURE_BIN".into(),
        harness
            .join("node_modules/.cache/probierz/screen-capture-kit")
            .to_string_lossy()
            .into_owned(),
    );
    if record {
        env.insert("PROBIERZ_RECORD".into(), "1".into());
    }
    let spec = if byk {
        Some("byk-auth.e2e.ts".into())
    } else {
        configured_spec
    };
    if let Some(spec) = &spec {
        if !byk {
            env.insert("PROBIERZ_SPEC".into(), spec.clone());
        }
    }
    if let Ok(binary) = std::env::current_exe() {
        env.insert("PROBIERZ_BIN".into(), binary.to_string_lossy().into_owned());
    }
    let resources = crate::evidence::resources_for(name, &opts.env);
    let lease = match crate::evidence::acquire_resources_wait(
        harness,
        &resources,
        &run_id,
        opts.resource_wait_ms,
    ) {
        Ok(lease) => lease,
        Err(error) => {
            let lock = json!({"error":error.to_string(),"resource":null,"owner":null});
            update_json(
                &manifest_path,
                &json!({"status":"blocked","completedAt":now_iso(),"resourceLock":lock,"artifacts":artifact_hashes(&artifacts,&manifest_path)?}),
            )?;
            let mut result = base.as_object().expect("object").clone();
            result.extend(
                json!({"ready":false,"skipped":true,"resourceLock":lock})
                    .as_object()
                    .expect("object")
                    .clone(),
            );
            return Ok(Value::Object(result));
        }
    };
    update_json(&manifest_path, &json!({"resources":lease.resources}))?;
    let lifecycle = app
        .as_ref()
        .and_then(|declaration| declaration.document.get("data"));
    let secrets = secret_values(&opts.env);
    let seed = run_data_command(
        harness,
        lifecycle.and_then(|value| value.get("seed")),
        &env,
        &secrets,
        &stdout_path,
        &stderr_path,
    );
    if seed.get("ok").and_then(Value::as_bool) == Some(true) {
        if let Some(seed_env) = seed.pointer("/result/env").and_then(Value::as_object) {
            let mut seeded_values = BTreeMap::new();
            for (name, value) in seed_env {
                let value = value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string());
                seeded_values.insert(name.clone(), value.clone());
                opts.env.insert(name.clone(), value.clone());
                env.insert(name.clone(), value);
            }
            let updated_conditions = run_conditions(record, &opts.env);
            base.as_object_mut()
                .expect("object")
                .insert("conditions".into(), updated_conditions.clone());
            let mut public_seed = seed.get("result").cloned().unwrap_or_else(|| json!({}));
            public_seed
                .as_object_mut()
                .expect("seed object")
                .insert("env".into(), redacted_environment(&seeded_values));
            update_json(
                &manifest_path,
                &json!({ "conditions": updated_conditions, "seed": public_seed }),
            )?;
        }
    }
    if seed.get("ok").and_then(Value::as_bool) != Some(true) {
        let cleanup = run_data_command(
            harness,
            lifecycle.and_then(|value| value.get("cleanup")),
            &env,
            &secrets,
            &stdout_path,
            &stderr_path,
        );
        let error = seed.get("error").and_then(Value::as_str).unwrap_or("");
        let validation = json!({"ok":false,"error":format!("seed failed: {error}")});
        let mut result = base.as_object().expect("object").clone();
        result.extend(json!({"ready":true,"skipped":false,"command":null,"spec":spec,"exitCode":1,"signal":null,"timedOut":false,"passed":false,"durationMs":Utc::now().timestamp_millis()-started_date.timestamp_millis(),"reportValidation":validation,"setupError":error,"cleanup":cleanup,"stdoutTail":"","stderrTail":error}).as_object().expect("object").clone());
        update_json(
            &manifest_path,
            &json!({"status":"failed","completedAt":now_iso(),"setupError":error,"cleanup":cleanup,"reportValidation":validation}),
        )?;
        return Ok(Value::Object(result));
    }
    let data_seeded = lifecycle.and_then(|value| value.get("seed")).is_some();
    let command_text = format!(
        "npm run {}{}",
        config.script,
        spec.as_ref()
            .filter(|_| !byk)
            .map(|spec| format!(" (PROBIERZ_SPEC={spec})"))
            .unwrap_or_default()
    );
    let timeout = if opts.timeout_ms > 0 {
        opts.timeout_ms
    } else {
        DEFAULT_TIMEOUT_MS
    };
    update_json(
        &manifest_path,
        &json!({"status":"running","command":command_text,"timeoutMs":timeout,"dataSeeded":data_seeded}),
    )?;
    let (exit_code, timed_out, out, err, performance, platform) = execute_suite(
        opts.local,
        &opts.host_selector,
        harness,
        config.script,
        &env,
        timeout,
        secret_values(&opts.env),
        &stdout_path,
        &stderr_path,
        name,
        &started_at,
        &artifacts,
    )?;
    let validation = report_identity(&report_path, &run_id, started_time);
    let cleanup = if data_seeded {
        run_data_command(
            harness,
            lifecycle.and_then(|value| value.get("cleanup")),
            &env,
            &secret_values(&opts.env),
            &stdout_path,
            &stderr_path,
        )
    } else {
        json!({"ok":true,"result":null})
    };
    let passed = exit_code == 0
        && !timed_out
        && validation.get("ok").and_then(Value::as_bool) == Some(true)
        && cleanup.get("ok").and_then(Value::as_bool) == Some(true);
    let mut result = base.as_object().expect("object").clone();
    result.extend(json!({"ready":true,"command":command_text,"spec":spec,"exitCode":exit_code,"signal":null,"timedOut":timed_out,"canceled":false,"passed":passed,"durationMs":Utc::now().timestamp_millis()-started_date.timestamp_millis(),"reportValidation":validation,"stdoutTail":out,"stderrTail":err,"cleanup":cleanup,"cleanupError":if cleanup.get("ok").and_then(Value::as_bool)==Some(true){Value::Null}else{cleanup.get("error").cloned().unwrap_or(Value::Null)},"performance":performance,"platformDiagnostics":platform}).as_object().expect("object").clone());
    update_json(
        &manifest_path,
        &json!({"status":if passed{"executed"}else{"failed"},"completedAt":now_iso(),"exitCode":exit_code,"signal":null,"timedOut":timed_out,"canceled":false,"durationMs":result.get("durationMs"),"reportValidation":validation,"cleanup":cleanup,"cleanupError":result.get("cleanupError"),"performance":performance,"platformDiagnostics":platform,"artifacts":artifact_hashes(&artifacts,&manifest_path)?}),
    )?;
    Ok(Value::Object(result))
}

fn complete_run(
    mut run: Value,
    analysis: Option<&Value>,
    analysis_error: Option<&str>,
) -> Result<Value, Failure> {
    if run.get("canceled").and_then(Value::as_bool) == Some(true) {
        return Ok(run);
    }
    let artifacts = PathBuf::from(
        run.get("artifactsDir")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let manifest_path = PathBuf::from(
        run.get("manifestPath")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let run_id = run
        .get("runId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let analysis_path = artifacts.join("analysis.json");
    let payload = if let Some(error) = analysis_error {
        json!({"runId":run_id,"error":error})
    } else {
        let mut value = analysis.cloned().unwrap_or_else(|| json!({}));
        value
            .as_object_mut()
            .expect("object")
            .insert("runId".into(), json!(run_id));
        value
    };
    write_json(&analysis_path, &payload)?;
    let capture_errors = analysis
        .and_then(|value| value.get("captureErrors"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let media = analysis
        .and_then(|value| value.get("media"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let missing: Vec<Value> = media
        .iter()
        .filter(|item| item.get("missing").and_then(Value::as_bool) == Some(true))
        .cloned()
        .collect();
    let crashes = analysis
        .and_then(|value| value.pointer("/diagnostics/crashes"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let valid = analysis_error.is_none()
        && analysis.is_some_and(|value| {
            value.get("runId").and_then(Value::as_str) == Some(&run_id)
                && js_number(value.get("total")) > 0.0
                && js_number(value.get("failed")) == 0.0
        })
        && capture_errors.is_empty()
        && missing.is_empty()
        && crashes.is_empty();
    let kinds: BTreeSet<&str> = media
        .iter()
        .filter_map(|item| item.get("kind").and_then(Value::as_str))
        .collect();
    let required = run
        .pointer("/conditions/record")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let present = !required
        || ["video", "trace", "screenshot"]
            .iter()
            .any(|kind| kinds.contains(kind));
    let mut errors: Vec<Value> = analysis_error
        .map(|error| vec![json!(error)])
        .unwrap_or_default();
    if analysis_error.is_none()
        && analysis
            .and_then(|value| value.get("runId"))
            .and_then(Value::as_str)
            != Some(&run_id)
    {
        errors.push(json!("analysis run ID mismatch"));
    }
    if analysis_error.is_none()
        && analysis
            .map(|value| js_number(value.get("total")) <= 0.0)
            .unwrap_or(true)
    {
        errors.push(json!("zero executed checks"));
    }
    if analysis_error.is_none()
        && analysis
            .map(|value| js_number(value.get("failed")) > 0.0)
            .unwrap_or(false)
    {
        errors.push(json!(format!(
            "{} failed checks",
            analysis
                .map(|value| js_number(value.get("failed")))
                .unwrap_or(0.0)
        )));
    }
    errors.extend(capture_errors.clone());
    errors.extend(missing.iter().map(|item| {
        json!(format!(
            "missing report-typed artifact: {}",
            item.get("file").and_then(Value::as_str).unwrap_or("")
        ))
    }));
    if !present {
        errors.push(json!(
            "recording requested but no report-typed capture was produced"
        ));
    }
    errors.extend(crashes.iter().map(|item| {
        json!(format!(
            "crash evidence: {}",
            item.get("message")
                .or_else(|| item.get("source"))
                .and_then(Value::as_str)
                .unwrap_or("unknown crash")
        ))
    }));
    let evidence = json!({"report":run.pointer("/reportValidation/ok").and_then(Value::as_bool).unwrap_or(false),"analysis":valid,"captureRequired":required,"capturePresent":present,"captureErrors":capture_errors,"missingMedia":missing.iter().filter_map(|item|item.get("file").cloned()).collect::<Vec<_>>(),"crashes":crashes,"errors":errors});
    let passed = run.get("passed").and_then(Value::as_bool).unwrap_or(false)
        && evidence.get("report").and_then(Value::as_bool) == Some(true)
        && valid
        && present;
    update_json(
        &manifest_path,
        &json!({"status":if passed{"passed"}else{"failed"},"completedAt":now_iso(),"exitCode":run.get("exitCode"),"timedOut":run.get("timedOut"),"reportValidation":run.get("reportValidation"),"evidence":evidence,"failure":Value::Null,"analysisPath":analysis_path,"artifacts":artifact_hashes(&artifacts,&manifest_path)?}),
    )?;
    let object = run.as_object_mut().expect("object");
    object.insert("passed".into(), json!(passed));
    object.insert("analysisPath".into(), json!(analysis_path));
    object.insert("evidence".into(), evidence);
    Ok(run)
}

fn run_registered_surface(harness: &Path, name: &str, opts: &RunArgs) -> Answer {
    let mut env = env_snapshot(&opts.env);
    let run_id = format!(
        "rust-{}-{}",
        name.replace(':', "-"),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let artifacts = opts
        .env
        .get("PROBIERZ_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| harness.join("test-results").join(&run_id));
    let report_path = artifacts.join("report.json");
    env.insert("PROBIERZ_RUN_ID".into(), run_id.clone());
    env.insert(
        "PROBIERZ_ARTIFACTS".into(),
        artifacts.to_string_lossy().into_owned(),
    );
    env.insert(
        "PROBIERZ_REPORT_PATH".into(),
        report_path.to_string_lossy().into_owned(),
    );
    // A registered journey is named by its title; an application-owned one is
    // named by its absolute path, and a path must arrive whole.
    let filter = opts.spec.as_deref().map(|value| {
        if value.contains('/') {
            value
        } else {
            value.strip_suffix(".spec.mjs").unwrap_or(value)
        }
    });
    let (report, code) = crate::specs::execute(
        name,
        harness,
        &artifacts,
        &report_path,
        filter,
        env,
        Some(run_id),
    )?;
    print_json(&report)?;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

pub fn run(harness: &Path, name: &str, args: &[String]) -> Answer {
    if target(name).is_none() {
        return Err(fail("cli.run", format!("unknown target: {name}")));
    }
    let opts = parse_run_args(args, false)?;
    // A flag that belongs to one target is refused before anything executes:
    // running a journey while silently ignoring what the operator asked for is
    // worse than refusing.
    if (opts.local || opts.seed_resend) && name != "mobile:ios:byk-auth" {
        return Err(fail(
            "cli.run",
            format!("--local and --seed-resend apply to mobile:ios:byk-auth, not {name}"),
        ));
    }
    if matches!(name, "tui" | "desktop:cua") {
        return run_registered_surface(harness, name, &opts);
    }
    if opts.seed_resend {
        return seed_byk_resend(harness, &opts.env);
    }
    let mut result = run_surface(
        harness,
        name,
        RunOptions {
            host_selector: byk_host_selector(opts.host.as_deref(), &opts.env),
            env: opts.env,
            local: opts.local,
            record: opts.record,
            timeout_ms: opts.timeout_ms,
            force: opts.force,
            spec: opts.spec,
            app_id: opts.app_id,
            kind: None,
            resource_wait_ms: opts.resource_wait_ms,
        },
    )?;
    if result.get("skipped").and_then(Value::as_bool) == Some(true) {
        print_json(&result)?;
        std::process::exit(3);
    }
    let mut analysis = Value::Null;
    if opts.analyze {
        match analyze_run(
            Path::new(
                result
                    .get("reportPath")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
            Some(Path::new(
                result
                    .get("artifactsDir")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )),
            result.get("tool").and_then(Value::as_str),
            opts.frames,
            result.get("runId").and_then(Value::as_str),
        ) {
            Ok(value) => {
                analysis = value;
                result = complete_run(result, Some(&analysis), None)?;
            }
            Err(error) => {
                analysis = json!({"error": error.detail});
                result = complete_run(result, None, Some(&error.detail))?;
            }
        }
    }
    let authoring = result
        .get("spec")
        .and_then(Value::as_str)
        .unwrap_or("")
        .contains(".author-staging-");
    let repair = if !result
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && !authoring
        && !opts.no_repair
        && std::env::var_os("PROBIERZ_REPAIR_SUPPRESS").is_none()
    {
        crate::authoring::repair_failed_run(
            harness,
            result
                .get("appId")
                .and_then(Value::as_str)
                .unwrap_or("probierz"),
            result.get("runId").and_then(Value::as_str),
            1,
            false,
        )?
    } else {
        Value::Null
    };
    let mut output = result.clone();
    output
        .as_object_mut()
        .expect("object")
        .insert("analysis".into(), analysis);
    output
        .as_object_mut()
        .expect("object")
        .insert("repair".into(), repair);
    let passed = result
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    print_json(&output)?;
    if !passed {
        std::process::exit(1);
    }
    Ok(())
}

fn orchestrate(
    harness: &Path,
    files: Option<Vec<String>>,
    reference: Option<&str>,
    opts: &RunArgs,
) -> Result<Value, Failure> {
    let selection = if let Some(files) = &files {
        affected_targets(harness, files)?
    } else {
        affected_from_git(harness, reference)?
    };
    let mut app_ids: BTreeSet<String> = selection
        .get("apps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|app| app.get("appId").and_then(Value::as_str).map(str::to_string))
        .collect();
    if let Some(app_id) = &opts.app_id {
        app_ids.insert(app_id.clone());
    }
    let mut checks = Vec::new();
    for app_id in app_ids {
        let (status, result) = match crate::authoring::validate_accessibility(harness, &app_id) {
            Ok(result) => {
                let status = if result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    "passed"
                } else {
                    "failed"
                };
                (status, result)
            }
            Err(error) => ("failed", json!({ "ok": false, "error": error.to_string() })),
        };
        checks.push(json!({
            "name": format!("accessibility:{app_id}"),
            "status": status,
            "result": result,
        }));
    }

    let mut results = Vec::new();
    for target in selection
        .get("targets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let run = run_surface(
            harness,
            target,
            RunOptions {
                local: false,
                host_selector: byk_host_selector(None, &BTreeMap::new()),
                env: opts.env.clone(),
                record: opts.record,
                timeout_ms: opts.timeout_ms,
                force: opts.force,
                spec: opts.spec.clone(),
                app_id: opts.app_id.clone(),
                kind: Some("pull-request".into()),
                resource_wait_ms: Some(opts.resource_wait_ms.unwrap_or(10 * 60 * 1000)),
            },
        )?;
        if run.get("skipped").and_then(Value::as_bool) == Some(true) {
            let remediation = run
                .pointer("/preflight/remediation")
                .cloned()
                .unwrap_or_else(|| {
                    run.pointer("/resourceLock/error")
                        .map(|value| json!([value]))
                        .unwrap_or_else(|| json!([]))
                });
            results.push(json!({
                "target": target,
                "runId": run["runId"],
                "status": "blocked",
                "artifactsDir": run["artifactsDir"],
                "manifestPath": run["manifestPath"],
                "missing": run.pointer("/preflight/missing").cloned().unwrap_or_else(|| json!([])),
                "remediation": remediation,
                "resourceLock": run.get("resourceLock").cloned().unwrap_or(Value::Null),
            }));
            continue;
        }
        let analyzed = analyze_run(
            Path::new(run.get("reportPath").and_then(Value::as_str).unwrap_or("")),
            Some(Path::new(
                run.get("artifactsDir")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )),
            run.get("tool").and_then(Value::as_str),
            opts.frames,
            run.get("runId").and_then(Value::as_str),
        );
        let (analysis, completed) = match analyzed {
            Ok(analysis) => {
                let complete = complete_run(run, Some(&analysis), None)?;
                (analysis, complete)
            }
            Err(error) => {
                let analysis = json!({ "error": error.detail });
                let complete = complete_run(run, None, Some(&error.detail))?;
                (analysis, complete)
            }
        };
        let passed = completed
            .get("passed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let repair =
            if !passed && !opts.no_repair && std::env::var_os("PROBIERZ_REPAIR_SUPPRESS").is_none()
            {
                crate::authoring::repair_failed_run(
                    harness,
                    completed
                        .get("appId")
                        .and_then(Value::as_str)
                        .unwrap_or("probierz"),
                    completed.get("runId").and_then(Value::as_str),
                    1,
                    false,
                )?
            } else {
                Value::Null
            };
        results.push(json!({
            "target": target,
            "runId": completed["runId"],
            "status": if passed { "passed" } else { "failed" },
            "exitCode": completed["exitCode"],
            "timedOut": completed["timedOut"],
            "durationMs": completed["durationMs"],
            "reportPath": completed["reportPath"],
            "artifactsDir": completed["artifactsDir"],
            "manifestPath": completed["manifestPath"],
            "analysisPath": completed["analysisPath"],
            "evidence": completed["evidence"],
            "analysis": analysis,
            "repair": repair,
        }));
    }
    let count = |status: &str| {
        results
            .iter()
            .filter(|result| result.get("status").and_then(Value::as_str) == Some(status))
            .count()
    };
    let check_count = |status: &str| {
        checks
            .iter()
            .filter(|check| check.get("status").and_then(Value::as_str) == Some(status))
            .count()
    };
    let summary = json!({
        "total": results.len() + checks.len(),
        "passed": count("passed") + check_count("passed"),
        "failed": count("failed") + check_count("failed"),
        "blocked": count("blocked"),
        "ran": count("passed") + count("failed"),
        "checks": checks.len(),
    });
    let affected = json!({
        "targets": selection["targets"],
        "crossCutting": selection["crossCutting"],
        "files": selection["files"],
        "apps": selection.get("apps").cloned().unwrap_or_else(|| json!([])),
    });
    let mut output = Map::new();
    if files.is_none() {
        output.insert(
            "ref".into(),
            selection
                .get("ref")
                .cloned()
                .or_else(|| reference.map(|value| json!(value)))
                .unwrap_or_else(|| json!("HEAD")),
        );
    }
    output.insert("affected".into(), affected);
    output.insert("results".into(), Value::Array(results));
    output.insert("checks".into(), Value::Array(checks));
    output.insert("summary".into(), summary);
    Ok(Value::Object(output))
}

pub fn ci(harness: &Path, args: &[String]) -> Answer {
    let opts = parse_run_args(args, true)?;
    let files = files_after_flag(args);
    let reference = args
        .first()
        .filter(|arg| !arg.starts_with("--"))
        .map(String::as_str);
    let result = orchestrate(harness, files, reference, &opts)?;
    let failed = result
        .pointer("/summary/failed")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let blocked = result
        .pointer("/summary/blocked")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    print_json(&result)?;
    if failed > 0 {
        std::process::exit(1);
    }
    if blocked > 0 {
        std::process::exit(3);
    }
    Ok(())
}

fn matrix_expand(dimensions: Option<&serde_yaml::Value>) -> Vec<BTreeMap<String, String>> {
    let mut cells = vec![BTreeMap::new()];
    let mut values: Vec<(&str, &serde_yaml::Value)> = dimensions
        .and_then(serde_yaml::Value::as_mapping)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| Some((key.as_str()?, value)))
        .collect();
    values.sort_by_key(|(name, _)| *name);
    for (name, items) in values {
        let mut expanded = Vec::new();
        for cell in &cells {
            for value in items.as_sequence().into_iter().flatten() {
                let mut next = cell.clone();
                next.insert(name.into(), yaml_string(value).unwrap_or_default());
                expanded.push(next);
            }
        }
        cells = expanded;
    }
    cells
}
fn cell_id(target: &str, env: &BTreeMap<String, String>) -> String {
    let stable = json!({"env":env,"target":target});
    hex::encode(Sha256::digest(stable.to_string().as_bytes()))[..16].into()
}

fn plan_matrix(harness: &Path, app_id: &str, profile: &str) -> Result<Value, Failure> {
    let declaration = manifest::load(harness, app_id)?;
    let policy = declaration
        .document
        .get("matrix")
        .and_then(|matrix| matrix.get(profile))
        .ok_or_else(|| {
            fail(
                "run.matrix",
                format!("app {app_id} has no {profile} matrix"),
            )
        })?;
    let surfaces = declaration
        .document
        .get("surfaces")
        .and_then(serde_yaml::Value::as_mapping)
        .ok_or_else(|| Failure::config("run.matrix", "surfaces missing"))?;
    let mut targets: Vec<String> = policy
        .get("targets")
        .and_then(serde_yaml::Value::as_sequence)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_yaml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| {
            surfaces
                .keys()
                .filter_map(serde_yaml::Value::as_str)
                .map(str::to_string)
                .collect()
        });
    targets.sort();
    let mut cells = Vec::new();
    for name in targets {
        let surface = surfaces
            .get(&serde_yaml::Value::String(name.clone()))
            .ok_or_else(|| {
                fail(
                    "run.matrix",
                    format!("matrix {profile} references unknown target: {name}"),
                )
            })?;
        let mut dimensions = policy
            .get("dimensions")
            .and_then(serde_yaml::Value::as_mapping)
            .cloned()
            .unwrap_or_default();
        if let Some(specific) = policy
            .get("surfaces")
            .and_then(|surfaces| surfaces.get(&name))
            .and_then(|surface| surface.get("dimensions"))
            .and_then(serde_yaml::Value::as_mapping)
        {
            dimensions.extend(specific.clone());
        }
        for axes in matrix_expand(Some(&serde_yaml::Value::Mapping(dimensions))) {
            let mut public_env = yaml_ordered_strings(surface.get("conditions"));
            for (name, value) in &axes {
                public_env.insert(name.clone(), Value::String(value.clone()));
            }
            let env: BTreeMap<String, String> = public_env
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_string())))
                .collect();
            let index = cells.len();
            let spec = policy
                .get("surfaces")
                .and_then(|surfaces| surfaces.get(&name))
                .and_then(|surface| surface.get("spec"))
                .and_then(serde_yaml::Value::as_str)
                .or_else(|| surface.get("spec").and_then(serde_yaml::Value::as_str));
            let mut journeys = manifest::surface_journeys(surface, &env);
            journeys.sort();
            cells.push(json!({
                "index": index,
                "cellId": cell_id(&name, &env),
                "target": name,
                "spec": spec,
                "journeys": journeys,
                "axes": axes,
                "env": Value::Object(public_env),
            }));
        }
    }
    let max = policy
        .get("maxCells")
        .and_then(serde_yaml::Value::as_u64)
        .unwrap_or(128) as usize;
    if cells.len() > max {
        return Err(fail(
            "run.matrix",
            format!(
                "matrix {profile} expands to {} cells (max {max})",
                cells.len()
            ),
        ));
    }
    let frames = policy
        .get("frames")
        .and_then(serde_yaml::Value::as_f64)
        .or_else(|| {
            policy
                .get("frames")
                .and_then(serde_yaml::Value::as_u64)
                .map(|value| value as f64)
        })
        .unwrap_or(0.0);
    Ok(json!({
        "schemaVersion": 1,
        "appId": app_id,
        "owner": declaration.document.get("owner").and_then(serde_yaml::Value::as_str).unwrap_or(""),
        "profile": profile,
        "record": policy.get("record").and_then(serde_yaml::Value::as_bool) != Some(false),
        "frames": number(frames),
        "timeoutMs": policy.get("timeoutMs").and_then(serde_yaml::Value::as_u64).unwrap_or(0),
        "resourceWaitMs": policy.get("resourceWaitMs").and_then(serde_yaml::Value::as_u64).unwrap_or(10 * 60 * 1000),
        "maximumParallel": policy.get("maximumParallel").and_then(serde_yaml::Value::as_u64).unwrap_or(4).max(1),
        "minimumCellEvidence": policy.get("minimumCellEvidence").and_then(serde_yaml::Value::as_str).unwrap_or("E3"),
        "artifactEncryption": policy.get("artifactEncryption").and_then(serde_yaml::Value::as_str).unwrap_or("optional"),
        "removePlaintextAfterProtection": policy.get("removePlaintextAfterProtection").and_then(serde_yaml::Value::as_bool).unwrap_or(false),
        "release": policy.get("release").and_then(serde_yaml::Value::as_str),
        "cells": cells,
    }))
}

pub fn matrix(harness: &Path, app_id: &str, profile: &str, args: &[String]) -> Answer {
    let plan_only = args.iter().any(|arg| arg == "--plan");
    let release_at = args.iter().position(|arg| arg == "--release");
    let release = if let Some(index) = release_at {
        Some(value_after(args, index, "--release")?)
    } else {
        None
    };
    if profile == "release" && !plan_only && release.is_none() {
        return Err(fail(
            "cli.matrix",
            "release matrix execution needs --release <id>",
        ));
    }
    let mut env = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--plan" {
            index += 1;
            continue;
        }
        if args[index] == "--release" {
            index += 2;
            continue;
        }
        let Some((name, value)) = args[index].split_once('=') else {
            return Err(fail(
                "cli.matrix",
                format!("unexpected matrix argument: {}", args[index]),
            ));
        };
        env.insert(name.into(), value.into());
        index += 1;
    }

    let plan = plan_matrix(harness, app_id, profile)?;
    if plan_only {
        return print_json(&plan);
    }
    let effective = release.or_else(|| {
        plan.get("release")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    if profile == "release" && effective.is_none() {
        return Err(fail(
            "run.matrix",
            "release matrix execution needs a release ID",
        ));
    }
    let artifact_key = env
        .get("PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE")
        .cloned()
        .or_else(|| std::env::var("PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE").ok());
    if plan.get("artifactEncryption").and_then(Value::as_str) == Some("required")
        && artifact_key.is_none()
    {
        return Err(fail(
            "run.matrix",
            "matrix requires PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE",
        ));
    }
    env.remove("PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE");
    for cell in plan
        .get("cells")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for (name, value) in &env {
            if let Some(axis) = cell
                .get("axes")
                .and_then(Value::as_object)
                .and_then(|axes| axes.get(name))
                .and_then(Value::as_str)
            {
                if axis != value {
                    return Err(fail(
                        "run.matrix",
                        format!("matrix axis {name} cannot be overridden"),
                    ));
                }
            }
        }
    }

    let protect = |run: &Value| -> (Value, Value) {
        let Some(key) = artifact_key.as_deref() else {
            return (Value::Null, Value::Null);
        };
        let result = crate::evidence::protect_run(
            harness,
            app_id,
            run.get("runId").and_then(Value::as_str).unwrap_or_default(),
            Some(profile),
            Some(Path::new(key)),
            plan.get("removePlaintextAfterProtection")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        );
        match result {
            Ok(artifact) => (artifact, Value::Null),
            Err(error) => (Value::Null, Value::String(error.to_string())),
        }
    };

    let mut results = Vec::new();
    for cell in plan
        .get("cells")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mut cell_env: BTreeMap<String, String> = cell
            .get("env")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_string())))
            .collect();
        cell_env.extend(env.clone());
        if let Some(release) = &effective {
            cell_env.insert("PROBIERZ_RELEASE".into(), release.clone());
        }
        let run = run_surface(
            harness,
            cell.get("target").and_then(Value::as_str).unwrap_or(""),
            RunOptions {
                local: false,
                host_selector: byk_host_selector(None, &BTreeMap::new()),
                env: cell_env,
                record: plan.get("record").and_then(Value::as_bool).unwrap_or(true),
                timeout_ms: plan.get("timeoutMs").and_then(Value::as_u64).unwrap_or(0),
                force: false,
                spec: cell.get("spec").and_then(Value::as_str).map(str::to_string),
                app_id: Some(app_id.into()),
                kind: Some(profile.into()),
                resource_wait_ms: plan.get("resourceWaitMs").and_then(Value::as_u64),
            },
        )?;
        let mut public = cell.clone();
        if let Some(map) = public.get_mut("env").and_then(Value::as_object_mut) {
            for (name, value) in map.iter_mut() {
                if sensitive_key(name) {
                    *value = json!("[REDACTED]");
                }
            }
        }
        if run.get("skipped").and_then(Value::as_bool) == Some(true)
            || run.get("canceled").and_then(Value::as_bool) == Some(true)
        {
            let canceled = run
                .get("canceled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let (artifact, protection_error) = protect(&run);
            public.as_object_mut().expect("object").extend(
                json!({
                    "status": if canceled { "canceled" } else { "blocked" },
                    "run": run,
                    "protectedArtifact": artifact,
                    "protectionError": protection_error,
                })
                .as_object()
                .expect("object")
                .clone(),
            );
            results.push(public);
            continue;
        }

        let analyzed = analyze_run(
            Path::new(run.get("reportPath").and_then(Value::as_str).unwrap_or("")),
            Some(Path::new(
                run.get("artifactsDir")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )),
            run.get("tool").and_then(Value::as_str),
            plan.get("frames").and_then(Value::as_f64).unwrap_or(0.0),
            run.get("runId").and_then(Value::as_str),
        );
        let (analysis, completed) = match analyzed {
            Ok(analysis) => {
                let complete = complete_run(run, Some(&analysis), None)?;
                (analysis, complete)
            }
            Err(error) => {
                let analysis = json!({ "error": error.detail });
                let complete = complete_run(run, None, Some(&error.detail))?;
                (analysis, complete)
            }
        };
        let (artifact, protection_error) = protect(&completed);
        let passed = completed
            .get("passed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && protection_error.is_null();
        let level = if !completed
            .get("passed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "E0"
        } else if completed
            .pointer("/conditions/record")
            .and_then(Value::as_bool)
            == Some(true)
            && completed
                .pointer("/evidence/report")
                .and_then(Value::as_bool)
                == Some(true)
            && completed
                .pointer("/evidence/analysis")
                .and_then(Value::as_bool)
                == Some(true)
            && completed
                .pointer("/evidence/capturePresent")
                .and_then(Value::as_bool)
                == Some(true)
        {
            "E3"
        } else {
            "E2"
        };
        public.as_object_mut().expect("object").extend(
            json!({
                "status": if passed { "passed" } else { "failed" },
                "evidenceLevel": level,
                "run": completed,
                "analysis": analysis,
                "protectedArtifact": artifact,
                "protectionError": protection_error,
            })
            .as_object()
            .expect("object")
            .clone(),
        );
        results.push(public);
    }

    let count = |status: &str| {
        results
            .iter()
            .filter(|result| result.get("status").and_then(Value::as_str) == Some(status))
            .count()
    };
    let required = plan
        .get("minimumCellEvidence")
        .and_then(Value::as_str)
        .unwrap_or("E3");
    let rank = |level: &str| match level {
        "E0" => 0,
        "E1" => 1,
        "E2" => 2,
        "E3" => 3,
        _ => -1,
    };
    let evidence_satisfied = results.iter().all(|result| {
        rank(
            result
                .get("evidenceLevel")
                .and_then(Value::as_str)
                .unwrap_or(""),
        ) >= rank(required)
    });
    let passed = count("passed") == results.len() && evidence_satisfied;
    let output = json!({
        "schemaVersion": 1,
        "appId": app_id,
        "profile": profile,
        "release": effective,
        "generatedAt": now_iso(),
        "verdict": {
            "passed": passed,
            "evidenceLevel": if passed { json!("E4") } else { Value::Null },
            "minimumCellEvidence": required,
            "evidenceSatisfied": evidence_satisfied,
        },
        "summary": {
            "total": results.len(),
            "passed": count("passed"),
            "failed": count("failed"),
            "blocked": count("blocked"),
            "canceled": count("canceled"),
        },
        "results": results,
    });
    print_json(&output)?;
    if !passed {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glob_double_star_crosses_directories_but_star_does_not() {
        assert!(glob_matches("src/**/view.ts", "src/a/b/view.ts"));
        assert!(!glob_matches("src/*/view.ts", "src/a/b/view.ts"));
    }
}
