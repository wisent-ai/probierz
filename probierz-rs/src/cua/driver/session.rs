use serde_json::json;
use crate::cua::*;

/// Reaching the driver and starting what it should drive.
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

}
