use super::*;
pub(crate) const POLL: Duration = Duration::from_millis(500);
pub(crate) const SHELL_TIMEOUT: Duration = Duration::from_secs(45);
pub(crate) const CONTRACT_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const REPORT_TIMEOUT: Duration = Duration::from_secs(150);
pub(crate) const REQUIRED_REPORT_ENTRIES: [&str; 7] = [
    "functionality",
    "diagnostics",
    "cli",
    "gui",
    "documentation",
    "tests",
    "delivery",
];

#[derive(Clone)]
pub(crate) struct View {
    pub(crate) tree: String,
    pub(crate) snapshot_id: Option<String>,
    pub(crate) elements: Vec<Value>,
    pub(crate) snapshot: Snapshot,
}

pub(crate) struct Backend {
    pub(crate) contract: Value,
    pub(crate) session_path: PathBuf,
}

pub(crate) struct Recorded {
    pub(crate) session_id: String,
    pub(crate) report: Value,
    pub(crate) final_text: String,
}

pub(crate) fn required(context: &specs::Context, name: &str) -> Result<String, String> {
    context
        .optional(name)
        .ok_or_else(|| format!("{name} is required for the Jeden Desktop task-contract journey"))
}

pub(crate) fn required_path(
    context: &specs::Context,
    name: &str,
    file: bool,
) -> Result<PathBuf, String> {
    let value = required(context, name)?;
    let path = PathBuf::from(&value);
    if !path.is_absolute() {
        return Err(format!(
            "{name} must be an absolute path, received {value:?}"
        ));
    }
    if !path.exists() {
        return Err(format!("{name} does not exist: {}", path.display()));
    }
    if file && !path.is_file() {
        return Err(format!("{name} is not a file: {}", path.display()));
    }
    Ok(path)
}

pub(crate) fn require_remote(context: &specs::Context) -> Result<String, String> {
    let job_id = required(context, "WC_JOB_ID")?;
    if !Regex::new(r"^job-[0-9a-f]{24}$").unwrap().is_match(&job_id) {
        return Err("This journey refuses local execution: WC_JOB_ID must be the canonical ID supplied by a real Stado worker job".to_string());
    }
    if std::env::consts::OS != "macos" {
        return Err(
            "The Jeden Desktop task-contract journey requires the Stado-selected macOS GUI worker"
                .to_string(),
        );
    }
    this_host_target()?;
    if required(context, "PROBIERZ_APP_ID")? != "jeden-desktop" {
        return Err(
            "The native task-contract journey may only run for the registered jeden-desktop app"
                .to_string(),
        );
    }
    let journeys: HashSet<String> = required(context, "PROBIERZ_JOURNEYS")?
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    if !journeys.contains("task-contract") {
        return Err(
            "The registered task-contract journey must authorize this product-owned spec"
                .to_string(),
        );
    }
    required_path(context, "PROBIERZ_SOURCE_IDENTITY", true)?;
    required_path(context, "PROBIERZ_APP_SOURCE", false)?;
    Ok(job_id)
}

pub(crate) fn view(snapshot: Snapshot) -> View {
    View {
        tree: snapshot.tree.clone(),
        snapshot_id: snapshot.snapshot_id.clone(),
        elements: snapshot.elements.clone(),
        snapshot,
    }
}

pub(crate) fn observed_at() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    millis.to_string()
}

/// The Stado registry name of the host this journey runs on, as Stado itself
/// answers it (`stado registry self`). The registry, not this file, says which
/// hosts are in the fleet, so a host that is renamed or replaced is known
/// without a release of Probierz, and a machine that is no registry target is
/// refused with Stado's own reason.
pub(crate) fn this_host_target() -> Result<String, String> {
    let home = std::env::var("HOME")
        .map_err(|_| "HOME is required to find the Stado executable".to_string())?;
    let output = Command::new(PathBuf::from(home).join(".stado/bin/stado"))
        .args(["registry", "self", "--name-only"])
        .output()
        .map_err(|error| format!("cannot ask Stado which registry target this machine is: {error}"))?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || name.is_empty() {
        return Err(format!(
            "This journey runs only on a Stado fleet host; `stado registry self` answered: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(name)
}
