use super::*;
pub(crate) fn compile_idle_program(dir: &Path, target: &Path) -> Result<PathBuf, String> {
    let source = dir.join("idle.c");
    fs::write(
        &source,
        "#include <unistd.h>\nint main(void){for(;;){pause();}return 0;}\n",
    )
    .map_err(|error| format!("{}: {error}", source.display()))?;
    let compiled = Command::new("/usr/bin/cc")
        .args(["-O0", "-o"])
        .arg(target)
        .arg(&source)
        .output()
        .map_err(|error| format!("cannot compile the fixture service program (needs the macOS command line tools): {error}"))?;
    if !compiled.status.success() {
        return Err(format!(
            "cannot compile the fixture service program (needs the macOS command line tools): {}",
            String::from_utf8_lossy(&compiled.stderr)
        ));
    }
    set_executable(target)?;
    Ok(target.to_path_buf())
}

pub(crate) fn copy_program(from: &Path, to: &Path) -> Result<PathBuf, String> {
    fs::copy(from, to)
        .map_err(|error| format!("{} -> {}: {error}", from.display(), to.display()))?;
    set_executable(to)?;
    Ok(to.to_path_buf())
}

pub(crate) fn write_agent_plist(
    path: &Path,
    label: &str,
    program_args: &[&Path],
) -> Result<(), String> {
    let args = program_args
        .iter()
        .map(|argument| format!("        <string>{}</string>", argument.display()))
        .collect::<Vec<_>>()
        .join("\n");
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n    <key>Label</key>\n    <string>{label}</string>\n    <key>ProgramArguments</key>\n    <array>\n{args}\n    </array>\n    <key>RunAtLoad</key>\n    <true/>\n</dict>\n</plist>\n"
    );
    fs::write(path, plist).map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) fn bootstrap_agent(plist_path: &Path, label: &str) -> Result<u32, String> {
    let domain = launchd_domain()?;
    let loaded = Command::new("/bin/launchctl")
        .args(["bootstrap", &domain])
        .arg(plist_path)
        .output()
        .map_err(|error| format!("launchd refused the fixture job: {error}"))?;
    if !loaded.status.success() {
        return Err(format!(
            "launchd refused the fixture job: {}{}",
            String::from_utf8_lossy(&loaded.stderr),
            String::from_utf8_lossy(&loaded.stdout)
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(pid) = launchd_pid(label)? {
            return Ok(pid);
        }
        thread::sleep(Duration::from_millis(300));
    }
    Err(format!("launchd started no process for {label}"))
}

pub(crate) fn launchd_pid(label: &str) -> Result<Option<u32>, String> {
    let domain = launchd_domain()?;
    let printed = Command::new("/bin/launchctl")
        .args(["print", &format!("{domain}/{label}")])
        .output()
        .map_err(|error| format!("cannot inspect launchd job {label}: {error}"))?;
    if !printed.status.success() {
        return Ok(None);
    }
    let pid = String::from_utf8_lossy(&printed.stdout)
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("pid = "))
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 0);
    Ok(pid)
}

pub(crate) fn bootout_agent(label: &str) {
    if let Ok(domain) = launchd_domain() {
        let _ = Command::new("/bin/launchctl")
            .args(["bootout", &format!("{domain}/{label}")])
            .output();
    }
}

pub(crate) fn spawn_orphan(command: &str) -> Result<u32, String> {
    let spawned = Command::new("/bin/sh")
        .args(["-c", &format!("nohup {command} >/dev/null 2>&1 & echo $!")])
        .output()
        .map_err(|error| format!("could not start the unowned fixture process: {error}"))?;
    let pid = String::from_utf8_lossy(&spawned.stdout)
        .trim()
        .parse::<u32>()
        .unwrap_or(0);
    if pid == 0 {
        Err(format!(
            "could not start the unowned fixture process: {}",
            String::from_utf8_lossy(&spawned.stderr)
        ))
    } else {
        Ok(pid)
    }
}

pub(crate) fn alive(pid: u32) -> bool {
    Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(crate) fn stop(pid: Option<u32>) {
    if let Some(pid) = pid {
        let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
    }
}

pub(crate) fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let mut body = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    body.push(b'\n');
    write_private(path, &body).map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) fn ensure(condition: bool, reason: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(reason.into())
    }
}

pub(crate) fn array_contains_string(value: &Value, needle: &str) -> bool {
    value
        .as_array()
        .map(|items| items.iter().any(|item| item.as_str() == Some(needle)))
        .unwrap_or(false)
}
pub(crate) fn value_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn marker_slug(slug: &str) -> String {
    slug.to_uppercase().replace('-', "_")
}

pub(crate) fn launchd_domain() -> Result<String, String> {
    let output = Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .map_err(|error| format!("cannot determine launchd domain: {error}"))?;
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && !uid.is_empty() {
        Ok(format!("gui/{uid}"))
    } else {
        Err("cannot determine launchd domain".to_string())
    }
}

pub(crate) fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}
