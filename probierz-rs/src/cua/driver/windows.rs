use serde_json::json;
use crate::cua::*;

/// Windows, their geometry, and the sidebar rows inside them.
impl Driver {
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

    pub(crate) fn element_request(
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

    pub(crate) fn wait_for_window(&self, pid: u32) -> Result<App, String> {
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

    pub(crate) fn find_window(&self, pid: u32) -> Result<Option<Value>, String> {
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

    pub(crate) fn ensure_daemon(&self) -> Result<(), String> {
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

    pub(crate) fn probe_permissions(&self) -> Option<Value> {
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
