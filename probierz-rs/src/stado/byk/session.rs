use serde_json::json;
use crate::stado::*;
pub fn run_remote_byk_auth(request: RemoteBykRequest<'_>) -> Result<RemoteBykOutcome, Failure> {
    RECEIVED_SIGNAL.store(0, Ordering::SeqCst);
    install_byk_signal_handlers();
    require_local_kind(request.root, true, "Probierz root")?;
    require_local_kind(request.app_path, true, "APP_IOS")?;
    if !fs::metadata(request.socket_path)?.file_type().is_socket() {
        return Err(Failure::config(
            "byk.remote",
            "local OTP broker socket is unavailable",
        ));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Failure::config("byk.remote", "HOME is required"))?;
    assert_byk_host_available(&home)?;
    let target = resolve_byk_target(request.host_selector)?;
    retry_byk("Stado reachability check", || {
        sh_with_input(
            STADO_BIN,
            &[
                "host".into(),
                "ping".into(),
                target.registry_target.clone(),
                "--json".into(),
            ],
            &[],
        )
    })?;
    let source_files = source_file_list(request.root)?;
    if source_files.is_empty() {
        return Err(Failure::config(
            "byk.remote",
            "Probierz source set is empty",
        ));
    }
    let run_id = uuid_v4()?;
    let remote_run_relative = format!(".stado/work/runs/{run_id}");
    let run_root = target
        .remote_home
        .join(".stado")
        .join("work")
        .join("runs")
        .join(&run_id);
    let remote_source = run_root.join("probierz");
    let remote_app = run_root.join("Byk.app");
    let remote_socket = run_root.join("byk-otp.sock");
    let worker = remote_source.join("probierz-rs/target/release/probierz");
    let remote_port = byk_forward_port(&run_id);
    let bridge_token = uuid_v4()?;
    let bridge = BykLocalBridge::start(request.socket_path, &bridge_token)?;
    let forward_name = format!("probierz-byk-{}", run_id.replace('-', ""));
    let forward_args = vec![
        "host".into(),
        "forward-local".into(),
        target.registry_target.clone(),
        forward_name.clone(),
        "--remote-port".into(),
        remote_port.to_string(),
        "--local-port".into(),
        bridge.port().to_string(),
        "--json".into(),
    ];
    let mut forward_opened = false;
    let mut created = false;
    let result = (|| {
        retry_byk("Stado OTP forwarding channel", || {
            sh_with_input(STADO_BIN, &forward_args, &[])
        })?;
        forward_opened = true;
        retry_byk("dedicated-host preparation", || {
            sh_with_input(
                STADO_BIN,
                &[
                    "host".into(),
                    "exec".into(),
                    target.registry_target.clone(),
                    "--".into(),
                    "mkdir".into(),
                    "-p".into(),
                    ".stado/work/runs".into(),
                ],
                &[],
            )
        })?;
        created = true;
        retry_byk("Probierz source delivery", || {
            sh_with_input(
                STADO_BIN,
                &[
                    "host".into(),
                    "deliver".into(),
                    target.registry_target.clone(),
                    request.root.display().to_string(),
                    format!("{remote_run_relative}/probierz"),
                    "--files-from".into(),
                    "-".into(),
                    "--json".into(),
                ],
                &source_files,
            )
        })?;
        retry_byk("Byk app delivery", || {
            sh_with_input(
                STADO_BIN,
                &[
                    "host".into(),
                    "deliver".into(),
                    target.registry_target.clone(),
                    request.app_path.display().to_string(),
                    format!("{remote_run_relative}/Byk.app"),
                    "--json".into(),
                ],
                &[],
            )
        })?;
        retry_byk("dedicated-host worker build", || {
            sh_with_input(
                STADO_BIN,
                &[
                    "host".into(),
                    "build".into(),
                    target.registry_target.clone(),
                    "--manifest-path".into(),
                    remote_source
                        .join("probierz-rs/Cargo.toml")
                        .display()
                        .to_string(),
                    "--bin".into(),
                    "probierz".into(),
                    "--json".into(),
                ],
                &[],
            )
        })?;
        let config = json!({
            "runRoot": run_root,
            "sourceRoot": remote_source,
            "appPath": remote_app,
            "socketPath": remote_socket,
            "otpPort": remote_port,
            "bridgeToken": bridge_token,
            "recipient": request.recipient,
            "iosDevice": request.ios_device,
            "iosVersion": request.ios_version,
        });
        let mut input = serde_json::to_vec(&config)?;
        input.push(b'\n');
        let output = sh_with_input(
            STADO_BIN,
            &[
                "host".into(),
                "run-attached".into(),
                target.registry_target.clone(),
                "--program".into(),
                worker.display().to_string(),
                "--arg".into(),
                "stado".into(),
                "--arg".into(),
                "byk-auth-worker".into(),
            ],
            &input,
        );
        if output.status == Some(255) {
            return Err(Failure::unavailable(
                "byk.remote",
                "dedicated-host worker transport failed",
            ));
        }
        let received = RECEIVED_SIGNAL.load(Ordering::SeqCst);
        Ok(RemoteBykOutcome {
            code: if received == 0 { output.status } else { None },
            signal: if received == 0 {
                output.signal
            } else {
                Some(received)
            },
        })
    })();
    if created {
        let _ = sh_with_input(
            STADO_BIN,
            &[
                "host".into(),
                "remove-run-directory".into(),
                target.registry_target.clone(),
                run_root.display().to_string(),
                "--json".into(),
            ],
            &[],
        );
    }
    if forward_opened {
        let _ = sh_with_input(
            STADO_BIN,
            &[
                "host".into(),
                "forward-close".into(),
                target.registry_target.clone(),
                forward_name,
                "--json".into(),
            ],
            &[],
        );
    }
    drop(bridge);
    match &result {
        Ok(_) => clear_byk_quarantine(&home)?,
        Err(failure) if failure.code == Code::Unavailable => {
            quarantine_byk_host(&home, request.host_selector, &failure.detail)?
        }
        Err(_) => {}
    }
    result
}

