use serde_json::json;
use crate::evidence::*;
pub fn resources_for(target: &str, env: &BTreeMap<String, String>) -> Vec<String> {
    let value = |name: &str| {
        env.get(name)
            .filter(|value| !value.is_empty())
            .map(String::as_str)
    };
    let mut resources = match target {
        "mobile:ios" => vec![
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

/// Takes one resource's lock file and blocks until the holder releases it.
/// The kernel drops an advisory lock when its holder's process exits, so a
/// crashed run never leaves a stale lease and nothing polls (cli.md rule 8).
pub(crate) fn acquire_one(harness: &Path, resource: &str, owner: &str) -> Result<fs::File, Failure> {
    use fs2::FileExt;
    use std::io::{Seek, SeekFrom, Write};
    let lock_root = harness.join("test-results").join(".locks");
    fs::create_dir_all(&lock_root)?;
    let path = lock_root.join(format!("{}.lock", lock_name(resource)));
    let mut file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    file.lock_exclusive().map_err(|error| {
        Failure::unavailable(
            "evidence.lock",
            format!("could not lock resource {resource} at {}: {error}", path.display()),
        )
    })?;
    // The holder's identity, for an operator reading who has the device now.
    let holder = json!({ "schemaVersion": 2, "resource": resource, "runId": owner, "pid": std::process::id(), "acquiredAt": now_iso() });
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(holder.to_string().as_bytes())?;
    Ok(file)
}

/// Held resources; dropping the lease closes the files and releases them.
pub struct ResourceLease {
    pub resources: Vec<String>,
    files: Vec<fs::File>,
}

impl ResourceLease {
    pub fn release(&mut self) {
        self.files.clear();
    }
}

/// Locks every resource in sorted order, so two runs that need overlapping
/// sets cannot deadlock. Returns once all are held.
pub fn acquire_resources(
    harness: &Path,
    resources: &[String],
    owner: &str,
) -> Result<ResourceLease, Failure> {
    let mut unique = resources.to_vec();
    unique.sort();
    unique.dedup();
    let mut files = Vec::with_capacity(unique.len());
    for resource in &unique {
        files.push(acquire_one(harness, resource, owner)?);
    }
    Ok(ResourceLease {
        resources: unique,
        files,
    })
}

// Provider-neutral object listing. Kept public for read-side projections.
