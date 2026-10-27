use crate::stado::*;
use std::path::Component;

/// The exit status git answers when it succeeded.
/// <https://pubs.opengroup.org/onlinepubs/9799919799/functions/exit.html>
const EXIT_SUCCESS: i32 = 0;

/// The NUL that separates the listed paths, as `git ls-files -z` separates them.
/// <https://git-scm.com/docs/git-ls-files#Documentation/git-ls-files.txt--z>
const SEPARATOR: u8 = b'\0';

/// Whether a path holds credentials that never leave this machine: an `.env`
/// file or directory, or a Probierz credential file (`probierz-*.json`).
fn holds_secrets(path: &Path) -> bool {
    path.components()
        .any(|part| part.as_os_str().to_string_lossy().starts_with(".env"))
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("probierz-") && name.ends_with(".json"))
}

/// The files of the source tree under `root` that a run packs: what git
/// tracks or would track (its own ignore rules decide what is left out),
/// without credential files, as NUL-separated relative paths. A tracked file
/// deleted from the working tree is not packed; any other file that cannot
/// be read is refused by name rather than left out of the pack unseen.
pub fn source_file_list(root: &Path) -> Result<Vec<u8>, Failure> {
    let output = sh(
        "git",
        &[
            "-C".into(),
            root.display().to_string(),
            "ls-files".into(),
            "--cached".into(),
            "--others".into(),
            "--exclude-standard".into(),
            "-z".into(),
        ],
        None,
    );
    if output.status != Some(EXIT_SUCCESS) {
        return Err(Failure::config(
            "run.source",
            format!("git ls-files in {}: {}", root.display(), process_text(&output)),
        ));
    }
    let mut files: Vec<String> = Vec::new();
    for relative in output.stdout.split(char::from(SEPARATOR)).filter(|entry| !entry.is_empty()) {
        let path = Path::new(relative);
        if path.components().any(|part| matches!(part, Component::ParentDir)) {
            return Err(Failure::config(
                "run.source",
                format!("git ls-files in {} listed {relative}, a path outside the tree", root.display()),
            ));
        }
        if holds_secrets(path) {
            continue;
        }
        match fs::symlink_metadata(root.join(path)) {
            Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
                files.push(relative.to_string())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Failure::config(
                    "run.source",
                    format!("{} is listed by git but cannot be read: {error}", root.join(path).display()),
                ))
            }
        }
    }
    if root.join("package-lock.json").exists() && !files.iter().any(|path| path == "package-lock.json") {
        files.push("package-lock.json".into());
    }
    files.sort();
    let mut answer = Vec::new();
    for file in files {
        answer.extend(file.as_bytes());
        answer.push(SEPARATOR);
    }
    Ok(answer)
}
