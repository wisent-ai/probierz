use crate::run::*;
pub(crate) struct Captured {
    pub(crate) status: Option<ExitStatus>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) error: Option<String>,
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
        .stdin(Stdio::inherit())
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
    match stado_wait::output(&mut command) {
        Ok(output) => Captured {
            status: Some(output.status),
            stdout: output.stdout,
            stderr: output.stderr,
            error: None,
        },
        Err(error) => Captured {
            status: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            error: Some(format!(
                "cannot capture {program} in {}: {error}",
                match cwd {
                    Some(path) => path.display().to_string(),
                    None => "the caller's directory".to_string(),
                }
            )),
        },
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
    let result = capture_text(program, args, None);
    result.error.is_none() && result.status.is_some_and(|status| status.success())
}

pub(crate) fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
/// `$HOME`, else the home the user database records. With neither it stops
/// with the cause instead of answering `.`, which put run state and evidence
/// under whatever directory Probierz started from.
pub(crate) fn home_dir() -> PathBuf {
    std::env::home_dir().expect("neither HOME nor the user database names a home directory")
}
