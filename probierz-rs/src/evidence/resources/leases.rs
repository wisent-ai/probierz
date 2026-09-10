use serde_json::json;
use crate::evidence::*;
pub fn resources_for(target: &str, env: &BTreeMap<String, String>) -> Vec<String> {
    let value = |name: &str| {
        env.get(name)
            .filter(|value| !value.is_empty())
            .map(String::as_str)
    };
    let mut resources = match target {
        "mobile:ios" | "mobile:ios:byk-auth" => vec![
            format!(
                "device:ios:{}:{}",
                value("IOS_DEVICE").unwrap_or("default"),
                value("IOS_VERSION").unwrap_or("default"),
            ),
            "port:4723".into(),
        ],
        "mobile:android" => vec![
            format!(
                "device:android:{}:{}",
                value("ANDROID_DEVICE").unwrap_or("default"),
                value("ANDROID_VERSION").unwrap_or("default"),
            ),
            "port:4723".into(),
        ],
        "desktop:mac" => vec![
            format!(
                "device:mac:{}",
                value("MAC_BUNDLE_ID")
                    .or_else(|| value("MAC_APP_PATH"))
                    .unwrap_or("host"),
            ),
            "port:4723".into(),
        ],
        "desktop:win" => vec![
            format!("device:win:{}", value("WIN_APP").unwrap_or("host")),
            "port:4723".into(),
        ],
        _ => Vec::new(),
    };
    resources.sort();
    resources.dedup();
    resources
}

pub(crate) fn lock_name(resource: &str) -> String {
    let mut label = segment(resource, "");
    label.truncate(80);
    format!("{label}-{}", &sha256_bytes(resource.as_bytes())[..12])
}

pub(crate) fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
    #[cfg(not(unix))]
    {
        pid == std::process::id()
    }
}

pub(crate) fn acquire_one(harness: &Path, resource: &str, owner: &str) -> Result<(PathBuf, Value), Failure> {
    let lock_root = harness.join("test-results").join(".locks");
    fs::create_dir_all(&lock_root)?;
    let directory = lock_root.join(lock_name(resource));
    for attempt in 0..2 {
        match fs::create_dir(&directory) {
            Ok(()) => {
                let owner_value = json!({ "schemaVersion": 1, "resource": resource, "runId": owner, "pid": std::process::id(), "acquiredAt": now_iso() });
                if let Err(error) =
                    write_new_json(&directory.join("owner.json"), &owner_value, true)
                {
                    let _ = fs::remove_dir_all(&directory);
                    return Err(error);
                }
                return Ok((directory, owner_value));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let current = try_json_file(&directory.join("owner.json"));
                let stale = match current.as_ref() {
                    Some(owner) => {
                        !process_alive(owner.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32)
                    }
                    None => fs::metadata(&directory)
                        .and_then(|metadata| metadata.modified())
                        .ok()
                        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                        .is_none_or(|age| age >= Duration::from_secs(30)),
                };
                if attempt == 0 && stale {
                    let tombstone = PathBuf::from(format!(
                        "{}.stale-{}-{}",
                        directory.display(),
                        std::process::id(),
                        Utc::now().timestamp_millis()
                    ));
                    if fs::rename(&directory, &tombstone).is_ok() {
                        let _ = fs::remove_dir_all(tombstone);
                        continue;
                    }
                }
                let detail = current
                    .as_ref()
                    .map(|value| {
                        format!(
                            "run {} (pid {})",
                            value
                                .get("runId")
                                .and_then(Value::as_str)
                                .unwrap_or("undefined"),
                            value.get("pid").and_then(Value::as_u64).unwrap_or(0)
                        )
                    })
                    .unwrap_or_else(|| "an unknown owner".into());
                return Err(Failure::unavailable(
                    "evidence.lock",
                    format!("resource locked: {resource} by {detail}"),
                ));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(Failure::unavailable(
        "evidence.lock",
        format!("could not acquire resource: {resource}"),
    ))
}

pub struct ResourceLease {
    pub resources: Vec<String>,
    directories: Vec<PathBuf>,
    owner: String,
}

impl ResourceLease {
    pub fn release(&mut self) {
        for directory in self.directories.drain(..).rev() {
            let current = try_json_file(&directory.join("owner.json"));
            if current.as_ref().is_some_and(|value| {
                value.get("runId").and_then(Value::as_str) == Some(&self.owner)
                    && value.get("pid").and_then(Value::as_u64) == Some(std::process::id() as u64)
            }) {
                let _ = fs::remove_dir_all(directory);
            }
        }
    }
}

impl Drop for ResourceLease {
    fn drop(&mut self) {
        self.release();
    }
}

pub fn acquire_resources_wait(
    harness: &Path,
    resources: &[String],
    owner: &str,
    timeout_ms: Option<u64>,
) -> Result<ResourceLease, Failure> {
    let mut unique = resources.to_vec();
    unique.sort();
    unique.dedup();
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(0));
    let started = Instant::now();
    loop {
        let mut acquired = Vec::new();
        let mut conflict = None;
        for resource in &unique {
            match acquire_one(harness, resource, owner) {
                Ok((directory, _)) => acquired.push(directory),
                Err(error) => {
                    conflict = Some(error);
                    break;
                }
            }
        }
        if let Some(error) = conflict {
            for directory in acquired.into_iter().rev() {
                let _ = fs::remove_dir_all(directory);
            }
            if started.elapsed() >= timeout {
                return Err(error);
            }
            thread::sleep(
                Duration::from_millis(250)
                    .min(timeout.saturating_sub(started.elapsed()))
                    .max(Duration::from_millis(1)),
            );
        } else {
            return Ok(ResourceLease {
                resources: unique,
                directories: acquired,
                owner: owner.to_string(),
            });
        }
    }
}

// Provider-neutral object listing. Kept public for read-side projections.
