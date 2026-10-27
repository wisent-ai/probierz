use crate::cua::*;
use serde_json::json;

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
            .ok_or_else(|| format!("window {window_id} of pid {pid} states no bounds"))?;
        crate::cua::bounds(bounds).ok_or_else(|| {
            format!("window {window_id} of pid {pid} states incomplete bounds: {bounds}")
        })
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

    pub(crate) fn find_window(&self, pid: u32) -> Result<Option<Value>, String> {
        let mut candidates: Vec<Value> = self
            .list_windows(Some(pid))?
            .into_iter()
            .filter(|window| {
                window.get("layer").and_then(Value::as_i64) == Some(NORMAL_WINDOW_LEVEL)
            })
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
            right_titled.cmp(&left_titled).then_with(|| {
                match (window_area(right), window_area(left)) {
                    (Some(right), Some(left)) => right.total_cmp(&left),
                    (right, left) => right.is_some().cmp(&left.is_some()),
                }
            })
        });
        Ok(candidates.into_iter().next())
    }

    pub(crate) fn ensure_daemon(&self) -> Result<(), String> {
        if self.socket.try_exists().map_err(|error| {
            format!(
                "inspect CuaDriver socket {}: {error}",
                self.socket.display()
            )
        })? {
            self.probe_permissions()?;
            return Ok(());
        }
        let parent = self.socket_directory()?;
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        let daemon_log = parent.join("probierz-daemon.log");
        // The launch is diagnosed from this log; one left by an earlier daemon
        // would be read as this launch's answer, so a log that stays refuses.
        match fs::remove_file(&daemon_log) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!(
                    "the previous CuaDriver daemon log {} could not be cleared: {error}",
                    daemon_log.display()
                ));
            }
            _ => {}
        }

        let launched = run_to_exit(
            Command::new("/usr/bin/open")
                .arg("-n")
                .arg("-g")
                .arg("-a")
                .arg("CuaDriver")
                .arg("--args")
                .arg("serve")
                .arg("--socket")
                .arg(&self.socket),
        )
        .map_err(|error| format!("CuaDriver app launch failed: {error}"))?;
        if !launched.status.success() {
            return Err(format!(
                "CuaDriver app launch failed: {}",
                output_detail(&launched).trim()
            ));
        }

        self.await_socket(&daemon_log)?;
        self.probe_permissions()?;
        Ok(())
    }

    pub(crate) fn probe_permissions(&self) -> Result<Value, String> {
        let output = run_to_exit(
            Command::new(&self.binary)
                .arg("call")
                .arg("check_permissions")
                .arg(r#"{"prompt":false}"#)
                .arg("--socket")
                .arg(&self.socket),
        )
        .map_err(|error| {
            format!(
                "run {} check_permissions on {}: {error}",
                self.binary,
                self.socket.display()
            )
        })?;
        if !output.status.success() {
            return Err(format!(
                "{} check_permissions on {} {}; {}",
                self.binary,
                self.socket.display(),
                crate::failure::ended(&output.status),
                output_detail(&output)
            ));
        }
        let payload: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
            format!(
                "decode CuaDriver check_permissions on {}: {error}",
                self.socket.display()
            )
        })?;
        // The answer nests its flags under `permissions`, or carries them at
        // the top level; the accessibility flag is read from that same object.
        let permissions = match payload.get("permissions") {
            Some(nested) => nested,
            None => &payload,
        };
        let accessibility = permissions.get("accessibility").and_then(Value::as_bool);
        match accessibility {
            Some(true) => Ok(permissions.clone()),
            Some(false) => Err(format!(
                "CuaDriver on {} reports Accessibility permission denied",
                self.socket.display()
            )),
            None => Err(format!(
                "CuaDriver check_permissions on {} returned no boolean accessibility state",
                self.socket.display()
            )),
        }
    }
}

/// The layer CuaDriver reports for an app's ordinary windows, as Core Graphics
/// numbers it (`kCGWindowLayer`).
const NORMAL_WINDOW_LEVEL: i64 = 0; // https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX10.8.sdk/System/Library/Frameworks/CoreGraphics.framework/Versions/A/Headers/CGWindowLevel.h ("#define kCGNormalWindowLevel CGWindowLevelForKey(kCGNormalWindowLevelKey) /* 0 */")
