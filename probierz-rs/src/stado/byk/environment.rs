use crate::stado::*;
pub(crate) fn protected_byk_child(root: &Path, candidate: &str, name: &str) -> Result<PathBuf, Failure> {
    let candidate = PathBuf::from(candidate);
    if !candidate.is_absolute() || !candidate.starts_with(root) || candidate == root {
        return Err(Failure::config(
            "byk.worker",
            format!("{name} must stay inside the protected run directory"),
        ));
    }
    Ok(candidate)
}

pub(crate) fn valid_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !value.chars().any(char::is_whitespace)
}

pub(crate) fn base_byk_environment() -> BTreeMap<String, String> {
    const NAMES: &[&str] = &[
        "PATH",
        "HOME",
        "TMPDIR",
        "USER",
        "SHELL",
        "LANG",
        "TERM",
        "COLORTERM",
        "FORCE_COLOR",
        "NO_COLOR",
        "CLICOLOR",
        "CLICOLOR_FORCE",
        "APPIUM_HOME",
        "DEVELOPER_DIR",
        "SDKROOT",
        "TOOLCHAINS",
        "XCODE_DEFAULT_TOOLCHAIN_OVERRIDE",
        "XCODE_DEVELOPER_USR_PATH",
        "XCODE_PRODUCT_BUILD_VERSION",
        "XCODE_TOOLCHAIN_PATH",
        "XCODE_VERSION_ACTUAL",
        "XCODE_VERSION_MAJOR",
        "XCODE_VERSION_MINOR",
        "XCODE_XCCONFIG_FILE",
        "IOS_DEVICE",
        "IOS_VERSION",
        "CI",
    ];
    let mut environment = BTreeMap::new();
    for (name, value) in std::env::vars() {
        if NAMES.contains(&name.as_str()) || name.starts_with("LC_") {
            environment.insert(name, value);
        }
    }
    let system = "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin";
    let path = environment
        .get("PATH")
        .map(|value| format!("{system}:{value}"))
        .unwrap_or_else(|| system.into());
    environment.insert("PATH".into(), path);
    environment
}

pub(crate) fn worker_status(
    command: &str,
    args: &[&str],
    cwd: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<i32, Failure> {
    let mut child = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| Failure::config("byk.worker", format!("could not start {command}")))?;
    ACTIVE_CHILD.store(child.id() as i32, Ordering::SeqCst);
    let status = child.wait()?;
    ACTIVE_CHILD.store(0, Ordering::SeqCst);
    Ok(status.code().unwrap_or(1))
}

