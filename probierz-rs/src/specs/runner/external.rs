use crate::specs::*;

/// A journey an application owns in its own repository.
///
/// Most journeys live in this crate. A product whose journey needs its own
/// tree — a manifest that points at an absolute path in that product's
/// checkout — keeps it there and declares the program to run. Probierz
/// executes that program with the run's environment and reads the canonical
/// report it writes, which is how the old Node runner treated an
/// application-owned spec, minus the assumption that it is JavaScript.
pub struct External {
    pub title: String,
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl External {
    /// What a manifest's `spec:` means when it is not a registered title.
    ///
    /// An absolute path is the program. A file this crate can identify as a
    /// script is run through the interpreter its shebang names, because the
    /// product that owns it decides its language, not this one.
    pub fn resolve(spec: &str) -> Result<Self, Failure> {
        let path = PathBuf::from(spec);
        if !path.is_absolute() {
            return Err(fail(
                "specs.external",
                format!(
                    "{spec} is neither a registered journey title nor an absolute path to a \
program that writes the canonical report"
                ),
            ));
        }
        let metadata = fs::metadata(&path)
            .map_err(|error| fail("specs.external", format!("{spec} cannot be read: {error}")))?;
        if !metadata.is_file() {
            return Err(fail("specs.external", format!("{spec} is not a file")));
        }
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| {
                name.strip_suffix(".probierz.spec.mjs")
                    .or_else(|| name.strip_suffix(".spec.mjs"))
                    .or_else(|| name.strip_suffix(".mjs"))
                    .unwrap_or(name)
                    .to_string()
            })
            .unwrap_or_else(|| spec.to_string());
        #[cfg(unix)]
        let executable = metadata.permissions().mode() & 0o111 != 0;
        #[cfg(not(unix))]
        let executable = true;
        if executable {
            return Ok(Self {
                title,
                program: path,
                args: Vec::new(),
            });
        }
        let interpreter = interpreter_of(&path)?;
        Ok(Self {
            title,
            args: vec![path.to_string_lossy().into_owned()],
            program: interpreter,
        })
    }

    pub(crate) fn run(&self, artifacts: &Path, env: &BTreeMap<String, String>) -> Result<(), String> {
        let output = Command::new(&self.program)
            .args(&self.args)
            .current_dir(artifacts)
            .envs(env)
            .output()
            .map_err(|error| format!("{} could not start: {error}", self.program.display()))?;
        if output.status.success() {
            return Ok(());
        }
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = if detail.trim().is_empty() {
            String::from_utf8_lossy(&output.stdout).to_string()
        } else {
            detail.to_string()
        };
        Err(format!(
            "{} exited {}: {}",
            self.program.display(),
            output.status.code().unwrap_or(-1),
            detail.trim()
        ))
    }
}

/// The interpreter a script's first line names. A journey this crate does not
/// own may be written in anything; refusing to guess is what keeps that true.
pub(crate) fn interpreter_of(path: &Path) -> Result<PathBuf, Failure> {
    let head = fs::read(path)
        .map_err(|error| fail("specs.external", format!("{}: {error}", path.display())))?;
    let first = String::from_utf8_lossy(&head[..head.len().min(256)]);
    let line = first.lines().next().unwrap_or_default();
    let rest = line.strip_prefix("#!").ok_or_else(|| {
        fail(
            "specs.external",
            format!(
                "{} is not executable and names no interpreter: make it executable or give it a shebang",
                path.display()
            ),
        )
    })?;
    let mut parts = rest.split_whitespace();
    let first = parts.next().unwrap_or_default();
    // `#!/usr/bin/env node` names the interpreter in its argument.
    if first.ends_with("/env") {
        if let Some(program) = parts.next() {
            return Ok(PathBuf::from(program));
        }
    }
    Ok(PathBuf::from(first))
}

