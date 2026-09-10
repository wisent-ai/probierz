use serde_json::json;
use crate::cua::*;

/// Reading a window and acting inside it.
impl Driver {
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

}
