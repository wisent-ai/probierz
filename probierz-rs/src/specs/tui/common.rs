use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::specs::Context;

pub struct Output {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }
    pub fn combined(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

pub fn required(context: &Context, name: &str, message: &str) -> Result<String, String> {
    context.optional(name).ok_or_else(|| message.to_string())
}

pub fn required_file(context: &Context, name: &str, missing: &str) -> Result<PathBuf, String> {
    let value = required(context, name, missing)?;
    let path = PathBuf::from(&value);
    if !path.is_absolute() {
        return Err(format!("{name} must be an absolute path"));
    }
    let metadata =
        fs::metadata(&path).map_err(|_| format!("{name} must identify a readable file"))?;
    if !metadata.is_file() {
        return Err(format!("{name} must identify a readable file"));
    }
    Ok(path)
}

pub fn scratch(prefix: &str) -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    Ok(path)
}

pub fn remove(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

pub fn run(
    command: &str,
    args: &[String],
    cwd: Option<&Path>,
    env: &BTreeMap<String, String>,
    remove_env: &[&str],
    input: Option<&str>,
    timeout: Duration,
) -> Result<Output, String> {
    let mut process = Command::new(command);
    process
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        process.current_dir(cwd);
    }
    process.envs(env);
    for name in remove_env {
        process.env_remove(name);
    }
    let mut child = process
        .spawn()
        .map_err(|error| format!("cannot start {command}: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("cannot capture {command} stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("cannot capture {command} stderr"))?;
    let stdout_buf = Arc::new(Mutex::new(Vec::new()));
    let stderr_buf = Arc::new(Mutex::new(Vec::new()));
    read_in_background(stdout, Arc::clone(&stdout_buf));
    read_in_background(stderr, Arc::clone(&stderr_buf));
    if let Some(text) = input {
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|error| format!("cannot write to {command}: {error}"))?;
        }
    } else {
        drop(child.stdin.take());
    }
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("cannot wait for {command}: {error}"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{command} {} timed out after {}ms",
                args.join(" "),
                timeout.as_millis()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    std::thread::sleep(Duration::from_millis(20));
    let stdout = String::from_utf8_lossy(
        &stdout_buf
            .lock()
            .map_err(|_| "stdout capture lock failed")?,
    )
    .into_owned();
    let stderr = String::from_utf8_lossy(
        &stderr_buf
            .lock()
            .map_err(|_| "stderr capture lock failed")?,
    )
    .into_owned();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn read_in_background<R: Read + Send + 'static>(mut reader: R, target: Arc<Mutex<Vec<u8>>>) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if let Ok(mut bytes) = target.lock() {
                        bytes.extend_from_slice(&buffer[..count]);
                    }
                }
            }
        }
    });
}

pub struct Service {
    child: Child,
    log: Arc<Mutex<Vec<u8>>>,
}

impl Service {
    pub fn spawn(
        command: &str,
        args: &[String],
        cwd: &Path,
        env: &BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let mut child = Command::new(command)
            .args(args)
            .current_dir(cwd)
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("cannot start {command}: {error}"))?;
        let log = Arc::new(Mutex::new(Vec::new()));
        if let Some(stdout) = child.stdout.take() {
            read_in_background(stdout, Arc::clone(&log));
        }
        if let Some(stderr) = child.stderr.take() {
            read_in_background(stderr, Arc::clone(&log));
        }
        Ok(Self { child, log })
    }
    pub fn exited(&mut self) -> Result<bool, String> {
        self.child
            .try_wait()
            .map(|value| value.is_some())
            .map_err(|e| e.to_string())
    }
    pub fn log(&self) -> String {
        self.log
            .lock()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    }
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn parse_json(text: &str, label: &str) -> Result<Value, String> {
    serde_json::from_str(text.trim()).map_err(|error| {
        format!("{label} did not return its canonical JSON value: {error}\n{text}")
    })
}

pub fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{} is not JSON: {error}", path.display()))
}

pub fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    set_private(path)
}

fn set_private(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub fn contains(text: &str, needle: &str, failure: impl Into<String>) -> Result<(), String> {
    if text.contains(needle) {
        Ok(())
    } else {
        Err(failure.into())
    }
}
pub fn excludes(text: &str, needle: &str, failure: impl Into<String>) -> Result<(), String> {
    if text.contains(needle) {
        Err(failure.into())
    } else {
        Ok(())
    }
}

pub fn env_map(
    pairs: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
}

pub fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

pub fn write_trace(context: &Context, file: &str, value: Value) -> Result<(), String> {
    let path = context.artifacts.join(file);
    write_json(&path, &value)?;
    context.media_typed("trace", path, "application/json");
    Ok(())
}
