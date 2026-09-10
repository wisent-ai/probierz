use crate::stado::*;
pub(crate) fn byk_auth_worker_inner() -> Result<i32, Failure> {
    install_byk_signal_handlers();
    let mut input = Vec::new();
    std::io::stdin().take(4097).read_to_end(&mut input)?;
    if input.len() > 4096 {
        return Err(Failure::config(
            "byk.worker",
            "remote Byk configuration is too large",
        ));
    }
    if input.last() != Some(&b'\n') || input[..input.len().saturating_sub(1)].contains(&b'\n') {
        return Err(Failure::config(
            "byk.worker",
            "remote Byk configuration must be one JSON line",
        ));
    }
    let config: BykWorkerConfig = serde_json::from_slice(&input)
        .map_err(|_| Failure::config("byk.worker", "remote Byk configuration is invalid JSON"))?;
    if [
        config.run_root.as_str(),
        config.source_root.as_str(),
        config.app_path.as_str(),
        config.socket_path.as_str(),
        config.bridge_token.as_str(),
        config.recipient.as_str(),
        config.ios_device.as_str(),
    ]
    .iter()
    .any(|value| value.is_empty())
    {
        return Err(Failure::config(
            "byk.worker",
            "remote Byk configuration has invalid schema",
        ));
    }
    let run_root = PathBuf::from(&config.run_root);
    let source_root = protected_byk_child(&run_root, &config.source_root, "sourceRoot")?;
    let app_path = protected_byk_child(&run_root, &config.app_path, "appPath")?;
    let socket_path = protected_byk_child(&run_root, &config.socket_path, "socketPath")?;
    if !valid_email(&config.recipient) {
        return Err(Failure::config(
            "byk.worker",
            "remote Byk recipient is invalid",
        ));
    }
    if config.ios_device.trim() != config.ios_device || config.ios_device.contains(['\r', '\n']) {
        return Err(Failure::config(
            "byk.worker",
            "remote iOS device name is invalid",
        ));
    }
    if !config.ios_version.is_empty()
        && !config
            .ios_version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(Failure::config(
            "byk.worker",
            "remote iOS version is invalid",
        ));
    }
    if !app_path.is_dir() {
        return Err(Failure::config(
            "byk.worker",
            "remote Byk app is unavailable",
        ));
    }
    let otp_bridge = BykRemoteBridge::start(&socket_path, config.otp_port, &config.bridge_token)?;
    let lock_path = run_root
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| Failure::config("byk.worker", "remote Byk run root is invalid"))?
        .join("byk-auth.lock");
    let npm_cache = run_root.join("npm-cache");
    let appium_home = run_root.join("appium-home");
    let modules = appium_home.join("node_modules");
    let driver = source_root
        .join("node_modules")
        .join("appium-xcuitest-driver");
    if fs::create_dir(&lock_path).is_err() {
        return Err(Failure::config(
            "byk.worker",
            "dedicated iOS host is already running a Byk device test",
        ));
    }
    fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o700))?;
    let result = (|| {
        let sdk = worker_status(
            "/usr/bin/xcrun",
            &["--sdk", "iphonesimulator", "--show-sdk-version"],
            &source_root,
            &base_byk_environment(),
        )?;
        if sdk != 0 {
            return Err(Failure::config(
                "byk.worker",
                "dedicated iOS host is missing the Xcode iOS Simulator SDK",
            ));
        }
        let mut install_env = base_byk_environment();
        install_env.insert("NPM_CONFIG_CACHE".into(), npm_cache.display().to_string());
        let install = worker_status(
            "/opt/homebrew/bin/npm",
            &[
                "ci",
                "--workspace",
                "packages/mobile",
                "--include-workspace-root=false",
            ],
            &source_root,
            &install_env,
        )?;
        if install != 0 {
            return Ok(install);
        }
        fs::create_dir_all(&modules)?;
        std::os::unix::fs::symlink(&driver, modules.join("appium-xcuitest-driver"))?;
        if npm_cache.exists() {
            fs::remove_dir_all(&npm_cache)?;
        }
        if RECEIVED_SIGNAL.load(Ordering::SeqCst) != 0 {
            return Ok(1);
        }
        let mut environment = base_byk_environment();
        environment.insert("PROBIERZ_SPEC".into(), "byk-auth.e2e.ts".into());
        environment.insert("BYK_OTP_SOCKET".into(), socket_path.display().to_string());
        environment.insert("BYK_TEST_EMAIL".into(), config.recipient.clone());
        environment.insert("APP_IOS".into(), app_path.display().to_string());
        environment.insert("APPIUM_HOME".into(), appium_home.display().to_string());
        environment.insert("IOS_DEVICE".into(), config.ios_device.clone());
        if !config.ios_version.is_empty() {
            environment.insert("IOS_VERSION".into(), config.ios_version.clone());
        }
        worker_status(
            "/opt/homebrew/bin/npm",
            &["run", "test:mobile:ios"],
            &source_root,
            &environment,
        )
    })();
    let _ = fs::remove_dir_all(&lock_path);
    drop(otp_bridge);
    let _ = fs::remove_dir_all(&run_root);
    result
}

