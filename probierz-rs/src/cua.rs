//! Native desktop automation through the `cua-driver` command line service.
//!
//! Targets obtained from an accessibility snapshot are snapshot-bound.  The
//! helpers below therefore keep the snapshot id and element token together and
//! never turn a tree match into an unscoped integer action.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const LAUNCH_WAIT: Duration = Duration::from_secs(8);
const POLL: Duration = Duration::from_millis(400);
const BUNDLED_DRIVER: &str = "/Applications/CuaDriver.app/Contents/MacOS/cua-driver";

#[derive(Clone, Debug)]
pub struct Driver {
    binary: String,
    socket: PathBuf,
}

#[derive(Clone, Debug)]
pub struct App {
    pub pid: u32,
    pub window_id: u64,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub tree: String,
    pub snapshot_id: Option<String>,
    pub elements: Vec<Value>,
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Driver {
    pub fn connect_config(binary: Option<String>, socket: Option<PathBuf>) -> Result<Self, String> {
        let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
            "HOME is required to locate the Probierz CuaDriver socket".to_string()
        })?;
        let socket = socket
            .or_else(|| std::env::var_os("CUA_DRIVER_SOCKET").map(PathBuf::from))
            .unwrap_or_else(|| home.join("Library/Caches/cua-driver/probierz.sock"));
        let binary = binary
            .or_else(|| std::env::var("CUA_DRIVER_BIN").ok())
            .unwrap_or_else(|| "cua-driver".to_string());
        let driver = Self { binary, socket };
        driver.ensure_daemon()?;
        Ok(driver)
    }

    pub fn call(&self, tool: &str, arguments: Value) -> Result<Value, String> {
        let output = Command::new(&self.binary)
            .arg("call")
            .arg(tool)
            .arg(arguments.to_string())
            .arg("--socket")
            .arg(&self.socket)
            .output()
            .map_err(|error| format!("cua-driver {tool} failed: {error}"))?;
        if !output.status.success() {
            let detail = output_detail(&output);
            return Err(format!(
                "cua-driver {tool} failed: {}",
                tail_chars(&detail, 400)
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    pub fn launch_app(
        &self,
        bundle_id: Option<&str>,
        name: Option<&str>,
        arguments: &[String],
        new_instance: bool,
    ) -> Result<App, String> {
        if bundle_id.is_none() && name.is_none() {
            return Err("launchCuaApp needs CUA_BUNDLE_ID or CUA_APP_NAME".to_string());
        }
        let mut request = serde_json::Map::new();
        if let Some(bundle_id) = bundle_id {
            request.insert("bundle_id".into(), Value::String(bundle_id.into()));
        } else if let Some(name) = name {
            request.insert("name".into(), Value::String(name.into()));
        }
        if !arguments.is_empty() {
            request.insert("additional_arguments".into(), json!(arguments));
        }
        if new_instance {
            request.insert("creates_new_application_instance".into(), Value::Bool(true));
        }
        let launched = self.call("launch_app", Value::Object(request))?;
        let pid = launched
            .get("pid")
            .or_else(|| launched.pointer("/app/pid"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                format!(
                    "launch_app returned no pid: {}",
                    tail_chars(&launched.to_string(), 300)
                )
            })? as u32;
        if let Some(window_id) =
            launched
                .get("windows")
                .and_then(Value::as_array)
                .and_then(|windows| {
                    windows
                        .iter()
                        .find_map(|window| window.get("window_id").and_then(Value::as_u64))
                })
        {
            return Ok(App { pid, window_id });
        }
        self.wait_for_window(pid)
    }

    /// Start an executable with its supplied environment without routing it
    /// through LaunchServices, then wait for its first real content window.
    pub fn launch_process(
        &self,
        executable: &Path,
        environment: &BTreeMap<String, String>,
        arguments: &[String],
    ) -> Result<App, String> {
        if executable.as_os_str().is_empty() {
            return Err(
                "launchCuaProcess needs CUA_APP_EXECUTABLE or an executable path".to_string(),
            );
        }
        let child = Command::new(executable)
            .args(arguments)
            .envs(environment)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("failed to spawn {}: {error}", executable.display()))?;
        let pid = child.id();
        // Dropping Child does not terminate it. The journey owns cleanup by pid.
        drop(child);
        self.wait_for_window(pid)
    }

    pub fn list_windows(&self, pid: Option<u32>) -> Result<Vec<Value>, String> {
        let listed = self.call("list_windows", Value::Object(Default::default()))?;
        Ok(listed
            .get("windows")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|window| {
                pid.is_none_or(|pid| window.get("pid").and_then(Value::as_u64) == Some(pid as u64))
            })
            .cloned()
            .collect())
    }

    pub fn snapshot(&self, pid: u32, window_id: u64) -> Result<Snapshot, String> {
        self.snapshot_to(pid, window_id, None)
    }

    pub fn snapshot_to(
        &self,
        pid: u32,
        window_id: u64,
        screenshot: Option<&Path>,
    ) -> Result<Snapshot, String> {
        let mut request = json!({ "pid": pid, "window_id": window_id });
        if let Some(file) = screenshot {
            request["screenshot_out_file"] = Value::String(file.to_string_lossy().into_owned());
        }
        let raw = self.call("get_window_state", request)?;
        Ok(Snapshot::from_value(raw))
    }

    pub fn screenshot(&self, pid: u32, window_id: u64, file: &Path) -> Result<Snapshot, String> {
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        let snapshot = self.snapshot_to(pid, window_id, Some(file))?;
        if !file.is_file() {
            return Err(format!(
                "cua-driver produced no screenshot at {}",
                file.display()
            ));
        }
        Ok(snapshot)
    }

    pub fn wait_for_text(
        &self,
        pid: u32,
        window_id: u64,
        needle: &str,
        timeout: Duration,
    ) -> Result<Snapshot, String> {
        let deadline = Instant::now() + timeout;
        let mut last = String::new();
        while Instant::now() < deadline {
            let snapshot = self.snapshot(pid, window_id)?;
            if snapshot.tree.contains(needle) {
                return Ok(snapshot);
            }
            last = snapshot.tree;
            thread::sleep(POLL);
        }
        Err(format!(
            "timed out waiting for {}; last tree (tail): {}",
            serde_json::to_string(needle).unwrap_or_else(|_| format!("\"{needle}\"")),
            tail_chars(&last, 600)
        ))
    }

    pub fn click_element(
        &self,
        pid: u32,
        window_id: u64,
        snapshot: &Snapshot,
        element: &Value,
    ) -> Result<Value, String> {
        let request = self.element_request(pid, window_id, snapshot, element)?;
        self.call("click", request)
    }

    pub fn click_pixel(
        &self,
        pid: u32,
        window_id: u64,
        x: f64,
        y: f64,
        foreground: bool,
    ) -> Result<Value, String> {
        let mut request = json!({
            "pid": pid,
            "window_id": window_id,
            "x": x.round() as i64,
            "y": y.round() as i64,
        });
        if foreground {
            request["delivery_mode"] = Value::String("foreground".into());
        }
        self.call("click", request)
    }
    pub fn click_screen(&self, pid: u32, x: f64, y: f64) -> Result<Value, String> {
        self.call(
            "click",
            json!({ "pid": pid, "x": x.round() as i64, "y": y.round() as i64 }),
        )
    }

    pub fn type_text(
        &self,
        pid: u32,
        window_id: u64,
        snapshot: &Snapshot,
        element: &Value,
        text: &str,
        foreground: bool,
    ) -> Result<Value, String> {
        let mut request = self.element_request(pid, window_id, snapshot, element)?;
        request["text"] = Value::String(text.into());
        if foreground {
            request["delivery_mode"] = Value::String("foreground".into());
        }
        self.call("type_text", request)
    }

    pub fn press_key(&self, pid: u32, window_id: Option<u64>, key: &str) -> Result<Value, String> {
        let mut request = json!({ "pid": pid, "key": key });
        if let Some(window_id) = window_id {
            request["window_id"] = json!(window_id);
        }
        self.call("press_key", request)
    }

    pub fn hotkey(&self, pid: u32, keys: &[&str]) -> Result<Value, String> {
        self.call("hotkey", json!({ "pid": pid, "keys": keys }))
    }

    pub fn bring_to_front(&self, pid: u32, window_id: u64) -> Result<Value, String> {
        self.call(
            "bring_to_front",
            json!({ "pid": pid, "window_id": window_id }),
        )
    }

    pub fn window_bounds(&self, pid: u32, window_id: u64) -> Result<Bounds, String> {
        let window = self
            .list_windows(Some(pid))?
            .into_iter()
            .find(|window| window.get("window_id").and_then(Value::as_u64) == Some(window_id))
            .ok_or_else(|| format!("window {window_id} not found for pid {pid}"))?;
        let bounds = window
            .get("bounds")
            .ok_or_else(|| format!("window {window_id} not found for pid {pid}"))?;
        Ok(Bounds {
            x: number(bounds, "x"),
            y: number(bounds, "y"),
            width: number(bounds, "width"),
            height: number(bounds, "height"),
        })
    }

    pub fn focus_sidebar(&self, pid: u32, window_id: u64, fraction: f64) -> Result<(), String> {
        let bounds = self.window_bounds(pid, window_id)?;
        self.click_screen(
            pid,
            bounds.x + bounds.width * fraction,
            bounds.y + bounds.height * 0.5,
        )?;
        Ok(())
    }

    pub fn select_sidebar_row(
        &self,
        pid: u32,
        window_id: u64,
        row_index: usize,
    ) -> Result<(), String> {
        self.focus_sidebar(pid, window_id, 0.12)?;
        for _ in 0..20 {
            self.press_key(pid, Some(window_id), "up")?;
        }
        for _ in 0..row_index {
            self.press_key(pid, Some(window_id), "down")?;
        }
        self.press_key(pid, Some(window_id), "return")?;
        Ok(())
    }

    pub fn quit_app(&self, pid: u32) {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }

    fn element_request(
        &self,
        pid: u32,
        window_id: u64,
        snapshot: &Snapshot,
        element: &Value,
    ) -> Result<Value, String> {
        if let Some(token) = element.get("element_token").and_then(Value::as_str) {
            return Ok(json!({ "pid": pid, "element_token": token }));
        }
        let index = element
            .get("element_index")
            .and_then(Value::as_u64)
            .ok_or_else(|| "element carries neither element_token nor element_index".to_string())?;
        let snapshot_id = snapshot
            .snapshot_id
            .as_deref()
            .ok_or_else(|| "snapshot carries no snapshot_id for its indexed element".to_string())?;
        Ok(json!({
            "pid": pid,
            "window_id": window_id,
            "element_index": index,
            "snapshot_id": snapshot_id,
        }))
    }

    fn wait_for_window(&self, pid: u32) -> Result<App, String> {
        let deadline = Instant::now() + LAUNCH_WAIT;
        while Instant::now() < deadline {
            if let Some(window) = self.find_window(pid)? {
                if let Some(window_id) = window.get("window_id").and_then(Value::as_u64) {
                    return Ok(App { pid, window_id });
                }
            }
            thread::sleep(POLL);
        }
        Err(format!("pid {pid} produced no window within 8000ms"))
    }

    fn find_window(&self, pid: u32) -> Result<Option<Value>, String> {
        let mut candidates: Vec<Value> = self
            .list_windows(Some(pid))?
            .into_iter()
            .filter(|window| window.get("layer").and_then(Value::as_i64) == Some(0))
            .collect();
        candidates.sort_by(|left, right| {
            let left_titled = left
                .get("title")
                .and_then(Value::as_str)
                .is_some_and(|title| !title.is_empty());
            let right_titled = right
                .get("title")
                .and_then(Value::as_str)
                .is_some_and(|title| !title.is_empty());
            right_titled
                .cmp(&left_titled)
                .then_with(|| window_area(right).total_cmp(&window_area(left)))
        });
        Ok(candidates.into_iter().next())
    }

    fn ensure_daemon(&self) -> Result<(), String> {
        let mut permissions = if self.socket.exists() {
            self.probe_permissions()
        } else {
            None
        };
        if permissions.is_none() {
            let daemon_binary = if self.binary != "cua-driver" {
                self.binary.clone()
            } else if cfg!(target_os = "macos") && Path::new(BUNDLED_DRIVER).exists() {
                BUNDLED_DRIVER.to_string()
            } else {
                "cua-driver".to_string()
            };
            let _ = run_timeout(
                Command::new(&daemon_binary)
                    .arg("stop")
                    .arg("--socket")
                    .arg(&self.socket),
                COMMAND_TIMEOUT,
            );
            let _ = fs::remove_file(&self.socket);
            let parent = self.socket.parent().unwrap_or_else(|| Path::new("."));
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
            let daemon_log = parent.join("probierz-daemon.log");
            let _ = fs::remove_file(&daemon_log);

            let launched = run_timeout(
                Command::new("/usr/bin/open")
                    .arg("-n")
                    .arg("-g")
                    .arg("-a")
                    .arg("CuaDriver")
                    .arg("--args")
                    .arg("serve")
                    .arg("--socket")
                    .arg(&self.socket),
                COMMAND_TIMEOUT,
            )
            .map_err(|error| format!("CuaDriver app launch failed: {error}"))?;
            if !launched.status.success() {
                return Err(format!(
                    "CuaDriver app launch failed: {}",
                    output_detail(&launched).trim()
                ));
            }

            let socket_deadline = Instant::now() + STARTUP_TIMEOUT;
            while Instant::now() < socket_deadline && !self.socket.exists() {
                thread::sleep(Duration::from_millis(100));
            }
            if !self.socket.exists() {
                let detail = fs::read_to_string(&daemon_log)
                    .ok()
                    .map(|text| tail_chars(&text, 2000))
                    .unwrap_or_default();
                return Err(if detail.is_empty() {
                    format!("CuaDriver did not create {}", self.socket.display())
                } else {
                    format!(
                        "CuaDriver did not create {}:\n{detail}",
                        self.socket.display()
                    )
                });
            }

            let probe_deadline = Instant::now() + STARTUP_TIMEOUT;
            while Instant::now() < probe_deadline {
                permissions = self.probe_permissions();
                if permissions.is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(250));
            }
        }
        if permissions.is_none() {
            return Err("Probierz CuaDriver daemon cannot use macOS Accessibility".to_string());
        }
        Ok(())
    }

    fn probe_permissions(&self) -> Option<Value> {
        let output = run_timeout(
            Command::new(&self.binary)
                .arg("call")
                .arg("check_permissions")
                .arg(r#"{"prompt":false}"#)
                .arg("--socket")
                .arg(&self.socket),
            COMMAND_TIMEOUT,
        )
        .ok()?;
        if !output.status.success() {
            return None;
        }
        let payload: Value = serde_json::from_slice(&output.stdout).ok()?;
        let permissions = payload.get("permissions").unwrap_or(&payload);
        let accessibility = payload
            .pointer("/permissions/accessibility")
            .or_else(|| payload.get("accessibility"))
            .and_then(Value::as_bool);
        (accessibility == Some(true)).then(|| permissions.clone())
    }
}

impl Snapshot {
    pub fn from_value(raw: Value) -> Self {
        let content = raw.get("structuredContent").unwrap_or(&raw);
        let tree = raw
            .get("tree_markdown")
            .or_else(|| content.get("tree_markdown"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let snapshot_id = content
            .get("snapshot_id")
            .or_else(|| raw.get("snapshot_id"))
            .and_then(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| value.as_u64().map(|id| id.to_string()))
            });
        let elements = content
            .get("elements")
            .or_else(|| raw.get("elements"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Self {
            tree,
            snapshot_id,
            elements,
        }
    }

    pub fn element_by_index(&self, index: u64) -> Option<&Value> {
        self.elements
            .iter()
            .find(|element| element.get("element_index").and_then(Value::as_u64) == Some(index))
    }

    /// An element by the token the snapshot itself minted. A token carries the
    /// snapshot it came from, so a stale token cannot silently address a
    /// different element after the window re-rendered.
    pub fn element_by_token(&self, token: &str) -> Option<&Value> {
        self.elements
            .iter()
            .find(|element| element.get("element_token").and_then(Value::as_str) == Some(token))
    }
}

pub fn element_index_of(tree: &str, needle: &str) -> Result<u64, String> {
    for line in tree.lines().filter(|line| line.contains(needle)) {
        if let Some(open) = line.find('[') {
            if let Some(close) = line[open + 1..].find(']') {
                if let Ok(index) = line[open + 1..open + 1 + close].parse() {
                    return Ok(index);
                }
            }
        }
    }
    Err(format!(
        "no indexed element matching {} in tree",
        json!(needle)
    ))
}

pub fn element_label(element: &Value) -> &str {
    element
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

pub fn element_role(element: &Value) -> &str {
    element
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

pub fn element_value(element: &Value) -> &str {
    element
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

pub fn element_frame(element: &Value) -> Option<Bounds> {
    let frame = element.get("frame")?;
    Some(Bounds {
        x: number(frame, "x"),
        y: number(frame, "y"),
        width: frame
            .get("w")
            .and_then(Value::as_f64)
            .unwrap_or_else(|| number(frame, "width")),
        height: frame
            .get("h")
            .and_then(Value::as_f64)
            .unwrap_or_else(|| number(frame, "height")),
    })
}

fn number(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn window_area(window: &Value) -> f64 {
    window
        .get("bounds")
        .map(|bounds| number(bounds, "width") * number(bounds, "height"))
        .unwrap_or(0.0)
}

fn output_detail(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        stderr.into_owned()
    } else {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

fn tail_chars(text: &str, count: usize) -> String {
    let mut characters: Vec<char> = text.chars().rev().take(count).collect();
    characters.reverse();
    characters.into_iter().collect()
}

fn run_timeout(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    let mut child: Child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|error| error.to_string()),
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(error) => return Err(error.to_string()),
        }
    }
    let _ = child.kill();
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    Err(if output_detail(&output).trim().is_empty() {
        "command timed out".to_string()
    } else {
        output_detail(&output)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_prefers_structured_content_and_resolves_bound_token() {
        let snapshot = Snapshot::from_value(json!({
            "tree_markdown": "- AXButton (Save) [7]",
            "structuredContent": {
                "snapshot_id": "s42",
                "elements": [{"element_index": 7, "element_token": "s42:7", "label": "Save"}]
            }
        }));
        assert_eq!(snapshot.snapshot_id.as_deref(), Some("s42"));
        assert_eq!(element_index_of(&snapshot.tree, "AXButton (Save)"), Ok(7));
        assert_eq!(snapshot.element_by_token("s42:7").unwrap()["label"], "Save");
    }
}
