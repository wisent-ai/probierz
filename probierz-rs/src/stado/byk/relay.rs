use crate::stado::*;

pub(crate) static ACTIVE_CHILD: AtomicI32 = AtomicI32::new(0);
pub(crate) static RECEIVED_SIGNAL: AtomicI32 = AtomicI32::new(0);
pub(crate) fn sh_with_input(command: &str, args: &[String], input: &[u8]) -> ProcessOutput {
    let mut child = match Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return ProcessOutput {
                command: command.into(),
                args: args.to_vec(),
                status: None,
                signal: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(error.to_string()),
            }
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(input) {
            let _ = child.kill();
            return ProcessOutput {
                command: command.into(),
                args: args.to_vec(),
                status: None,
                signal: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(error.to_string()),
            };
        }
    }
    ACTIVE_CHILD.store(child.id() as i32, Ordering::SeqCst);
    let result = child.wait_with_output();
    ACTIVE_CHILD.store(0, Ordering::SeqCst);
    match result {
        Ok(output) => ProcessOutput {
            command: command.into(),
            args: args.to_vec(),
            status: output.status.code(),
            signal: output.status.signal(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            error: None,
        },
        Err(error) => ProcessOutput {
            command: command.into(),
            args: args.to_vec(),
            status: None,
            signal: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(error.to_string()),
        },
    }
}

pub(crate) struct BykRemoteBridge {
    pub(crate) socket_path: PathBuf,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) thread: Option<thread::JoinHandle<()>>,
}

impl BykRemoteBridge {
    pub(crate) fn start(socket_path: &Path, port: u16, bridge_token: &str) -> Result<Self, Failure> {
        if port < 1024 || socket_path.exists() || !valid_bridge_token(bridge_token) {
            return Err(Failure::config(
                "byk.worker",
                "remote Byk OTP bridge configuration is invalid",
            ));
        }
        let listener = UnixListener::bind(socket_path).map_err(|error| {
            Failure::unavailable(
                "byk.worker",
                format!("could not bind the protected OTP socket: {error}"),
            )
        })?;
        fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let mut authentication = bridge_token.as_bytes().to_vec();
        authentication.push(b'\n');
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((unix, _)) => {
                        let authentication = authentication.clone();
                        thread::spawn(move || {
                            if let Ok(mut tcp) = TcpStream::connect(("127.0.0.1", port)) {
                                if tcp.write_all(&authentication).is_ok() {
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
            socket_path: socket_path.to_path_buf(),
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for BykRemoteBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.socket_path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BykWorkerConfig {
    pub(crate) run_root: String,
    pub(crate) source_root: String,
    pub(crate) app_path: String,
    pub(crate) socket_path: String,
    pub(crate) bridge_token: String,
    pub(crate) recipient: String,
    pub(crate) ios_device: String,
    pub(crate) otp_port: u16,
    pub(crate) ios_version: String,
}


unsafe extern "C" {
    pub(crate) fn signal(number: i32, handler: extern "C" fn(i32)) -> usize;
    pub(crate) fn kill(pid: i32, signal: i32) -> i32;
}

extern "C" fn byk_signal(number: i32) {
    let previous = RECEIVED_SIGNAL.swap(number, Ordering::SeqCst);
    let pid = ACTIVE_CHILD.load(Ordering::SeqCst);
    if pid > 0 {
        // SAFETY: kill is async-signal-safe and the PID is the currently
        // running direct child published by worker_status.
        unsafe {
            let _ = kill(pid, if previous == 0 { number } else { 9 });
        }
    }
}

pub(crate) fn install_byk_signal_handlers() {
    // SAFETY: the handler only touches atomics and calls async-signal-safe kill.
    unsafe {
        let _ = signal(2, byk_signal);
        let _ = signal(15, byk_signal);
        let _ = signal(1, byk_signal);
    }
}

pub(crate) fn byk_auth_worker() -> Answer {
    let code = match byk_auth_worker_inner() {
        Ok(code) => code,
        Err(failure) => {
            eprintln!("remote Byk runner: {}", failure.detail);
            1
        }
    };
    std::process::exit(code)
}

