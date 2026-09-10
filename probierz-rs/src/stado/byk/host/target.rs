use crate::stado::*;

#[derive(Debug)]
pub(crate) struct BykTarget {
    pub(crate) registry_target: String,
    pub(crate) remote_home: PathBuf,
}

pub(crate) fn resolve_byk_target(selector: &str) -> Result<BykTarget, Failure> {
    if selector.trim() != selector || !selector.starts_with("stado:") {
        return Err(Failure::config(
            "byk.remote",
            format!(
                "Stado could not resolve Byk host selector {selector:?}: expected a stado:<target> selector"
            ),
        ));
    }
    let registry_target = discovery::stado_host(selector)
        .and_then(|selected| selected.target.map(str::to_string))
        .or_else(|| {
            selector
                .strip_prefix("stado:")
                .filter(|target| !target.is_empty())
                .map(str::to_string)
        })
        .ok_or_else(|| {
            Failure::config(
                "byk.remote",
                format!(
                    "Stado could not resolve Byk host selector {selector:?}: selector has no registry target"
                ),
            )
        })?;
    let inventory = sh_with_input(
        STADO_BIN,
        &[
            "host".into(),
            "inventory".into(),
            registry_target.clone(),
            "--json".into(),
        ],
        &[],
    );
    if inventory.status != Some(0) {
        return Err(byk_resolution_failure(selector, &inventory));
    }
    let inventory: Value = serde_json::from_str(&inventory.stdout).map_err(|error| {
        Failure::config(
            "byk.remote",
            format!(
                "Stado could not resolve Byk host selector {selector:?}: invalid host inventory ({error})"
            ),
        )
    })?;
    if inventory.get("target").and_then(Value::as_str) != Some(registry_target.as_str()) {
        return Err(Failure::config(
            "byk.remote",
            format!(
                "Stado could not resolve Byk host selector {selector:?}: inventory named a different target"
            ),
        ));
    }
    if !inventory
        .get("declared_release_platform")
        .and_then(Value::as_str)
        .is_some_and(|platform| platform.starts_with("darwin-"))
    {
        return Err(Failure::config(
            "byk.remote",
            format!(
                "Stado could not place Byk host selector {selector:?}: target {registry_target:?} does not declare macOS"
            ),
        ));
    }
    let config = sh_with_input(
        STADO_BIN,
        &["host".into(), "config-show".into(), registry_target.clone()],
        &[],
    );
    if config.status != Some(0) {
        return Err(byk_resolution_failure(selector, &config));
    }
    let config: Value = serde_json::from_str(&config.stdout).map_err(|error| {
        Failure::config(
            "byk.remote",
            format!(
                "Stado could not resolve Byk host selector {selector:?}: invalid effective configuration ({error})"
            ),
        )
    })?;
    let config_file = config
        .get("file")
        .and_then(Value::as_str)
        .map(Path::new)
        .ok_or_else(|| {
            Failure::config(
                "byk.remote",
                format!(
                    "Stado could not resolve Byk host selector {selector:?}: effective configuration did not report its file"
                ),
            )
        })?;
    let remote_home = stado_home_from_config(config_file).ok_or_else(|| {
        Failure::config(
            "byk.remote",
            format!(
                "Stado could not resolve Byk host selector {selector:?}: effective configuration reported an invalid home"
            ),
        )
    })?;
    Ok(BykTarget {
        registry_target,
        remote_home,
    })
}

pub(crate) fn byk_resolution_failure(selector: &str, output: &ProcessOutput) -> Failure {
    let detail = process_text(output);
    Failure::config(
        "byk.remote",
        format!(
            "Stado could not resolve Byk host selector {selector:?}: {}",
            if detail.is_empty() {
                "Stado returned no diagnostic"
            } else {
                detail.as_str()
            }
        ),
    )
}

pub(crate) fn stado_home_from_config(file: &Path) -> Option<PathBuf> {
    if !file.is_absolute() || file.file_name()?.to_str()? != "config.json" {
        return None;
    }
    let stado = file.parent()?;
    let config = stado.parent()?;
    if stado.file_name()?.to_str()? != "stado" || config.file_name()?.to_str()? != ".config" {
        return None;
    }
    config.parent().map(Path::to_path_buf)
}

pub(crate) fn byk_forward_port(run_id: &str) -> u16 {
    let digest = Sha256::digest(run_id.as_bytes());
    20_000 + (u16::from_be_bytes([digest[0], digest[1]]) % 20_000)
}

pub(crate) fn valid_bridge_token(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

pub(crate) struct BykLocalBridge {
    pub(crate) port: u16,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) thread: Option<thread::JoinHandle<()>>,
}

impl BykLocalBridge {
    pub(crate) fn start(socket_path: &Path, bridge_token: &str) -> Result<Self, Failure> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
            Failure::unavailable(
                "byk.remote",
                format!("could not bind the local Stado OTP bridge: {error}"),
            )
        })?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let socket_path = socket_path.to_path_buf();
        let mut expected = bridge_token.as_bytes().to_vec();
        expected.push(b'\n');
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut tcp, _)) => {
                        let socket_path = socket_path.clone();
                        let expected = expected.clone();
                        thread::spawn(move || {
                            let _ = tcp.set_read_timeout(Some(Duration::from_secs(5)));
                            let mut received = vec![0_u8; expected.len()];
                            if tcp.read_exact(&mut received).is_ok()
                                && bool::from(received.ct_eq(&expected))
                            {
                                if let Ok(unix) = UnixStream::connect(socket_path) {
                                    relay_tcp_and_unix(tcp, unix);
                                }
                            }
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            port,
            stop,
            thread: Some(thread),
        })
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for BykLocalBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(crate) fn relay_tcp_and_unix(mut tcp: TcpStream, mut unix: UnixStream) {
    let Ok(mut tcp_reader) = tcp.try_clone() else {
        return;
    };
    let Ok(mut unix_writer) = unix.try_clone() else {
        return;
    };
    let upstream = thread::spawn(move || {
        let _ = std::io::copy(&mut tcp_reader, &mut unix_writer);
        let _ = unix_writer.shutdown(Shutdown::Write);
    });
    let _ = std::io::copy(&mut unix, &mut tcp);
    let _ = tcp.shutdown(Shutdown::Write);
    let _ = upstream.join();
}
