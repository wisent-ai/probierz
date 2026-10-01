use crate::run::*;
pub(crate) struct Captured {
    pub(crate) status: Option<ExitStatus>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) error: Option<String>,
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

/// Runs `program` to its own exit and captures both streams. There is no
/// deadline: the program's exit or its own error is the result (cli.md rule 8).
pub(crate) fn capture(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    env: Option<&BTreeMap<String, String>>,
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
    let status = match child.wait() {
        Ok(status) => Some(status),
        Err(_error) => {
            let _ = child.kill();
            None
        }
    };
    Captured {
        status,
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
        error: None,
    }
}

pub(crate) fn capture_text(program: &str, args: &[&str], cwd: Option<&Path>) -> Captured {
    capture(
        program,
        &args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>(),
        cwd,
        None,
    )
}

pub(crate) fn successful(program: &str, args: &[&str]) -> bool {
    capture_text(program, args, None)
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

