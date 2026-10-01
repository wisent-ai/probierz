use crate::run::*;
pub(crate) struct BykBroker {
    pub(crate) child: Child,
    pub(crate) directory: PathBuf,
    pub(crate) socket_path: PathBuf,
    pub(crate) recipient: String,
}

impl Drop for BykBroker {
    fn drop(&mut self) {
        // TERM asks the broker to stop; its exit ends the wait. No poll, no
        // 2 s window and no KILL escalation (cli.md rule 8).
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = Command::new("/bin/kill")
                .args(["-TERM", &self.child.id().to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = self.child.wait();
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

pub(crate) fn byk_broker_environment(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
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

pub(crate) fn byk_startup_error(message: &str, stderr: &Arc<Mutex<Vec<u8>>>) -> String {
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

pub(crate) fn valid_byk_recipient(value: &str) -> bool {
    value.len() <= 254
        && Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$")
            .expect("recipient regex")
            .is_match(value)
}

/// The login mailbox this target reads its one-time codes from, and the file
/// that holds the address a resend is sent from. Both are the target's, not a
/// caller's choice: a journey that authenticates a real account has exactly
/// one mailbox.
pub(crate) const BYK_MAILBOX: &str = "byk-ios-login";

