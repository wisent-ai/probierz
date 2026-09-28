//! A long-running process a journey starts, polls and stops, with its
//! stdout and stderr collected into one log.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use super::common::read_in_background;

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
