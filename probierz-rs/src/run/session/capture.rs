use crate::run::*;
pub(crate) struct Captured {
    pub(crate) status: Option<ExitStatus>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) error: Option<String>,
    pub(crate) timed_out: bool,
}

pub(crate) fn terminate_tree(child: &mut std::process::Child, hard: bool) {
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        command.args(["/PID", &child.id().to_string(), "/T"]);
        if hard {
            command.arg("/F");
        }
        let _ = command.stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(not(windows))]
    {
        let signal = if hard { "-KILL" } else { "-TERM" };
        let group = format!("-{}", child.id());
        let _ = Command::new("/bin/kill")
            .args([signal, &group])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !hard {
            let _ = child.kill();
        }
    }
}

pub(crate) fn capture(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    env: Option<&BTreeMap<String, String>>,
    timeout_ms: Option<u64>,
) -> Captured {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    if let Some(env) = env {
        command.envs(env);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Captured {
                status: None,
                stdout: Vec::new(),
                stderr: Vec::new(),
                error: Some(error.to_string()),
                timed_out: false,
            }
        }
    };
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(ref mut stream) = stdout {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    });
    let err_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(ref mut stream) = stderr {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    });
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if timeout_ms
                    .is_some_and(|timeout| started.elapsed() >= Duration::from_millis(timeout))
                {
                    timed_out = true;
                    terminate_tree(&mut child, false);
                    thread::sleep(Duration::from_millis(25));
                    if child.try_wait().ok().flatten().is_none() {
                        terminate_tree(&mut child, true);
                    }
                    break child.wait().ok();
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_error) => {
                break {
                    let _ = child.kill();
                    None
                }
            }
        }
    };
    Captured {
        status,
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
        error: None,
        timed_out,
    }
}

pub(crate) fn capture_text(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: Option<u64>,
) -> Captured {
    capture(
        program,
        &args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>(),
        cwd,
        None,
        timeout_ms,
    )
}

pub(crate) fn successful(program: &str, args: &[&str]) -> bool {
    capture_text(program, args, None, Some(PROBE_MS))
        .status
        .is_some_and(|status| status.success())
}

pub(crate) fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
pub(crate) fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

