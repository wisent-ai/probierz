use crate::specs::{self, tui::common};
use std::{path::PathBuf, process::Command};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let repo = PathBuf::from(context.optional("SKRZYNKA_REPO").unwrap_or_else(|| {
        "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skrzynka".into()
    }));
    if !repo.join("Cargo.toml").exists() {
        return Err(format!(
            "no skrzynka checkout at {}; set SKRZYNKA_REPO",
            repo.display()
        ));
    }
    if !repo.join("Cargo.lock").exists() {
        return Err(format!(
            "{} has no Cargo.lock, so --locked would have nothing to hold the build to",
            repo.display()
        ));
    }
    let output = Command::new("cargo")
        .args(["build", "--locked", "--all-targets"])
        .current_dir(&repo)
        .output()
        .map_err(|error| format!("cannot start cargo: {error}"))?;
    let build = common::Output {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    if !build.status.success() {
        return Err(format!(
            "cargo build --locked --all-targets failed with {}\n{}",
            build
                .code()
                .map_or_else(|| "signal".into(), |c| c.to_string()),
            if build.stderr.is_empty() {
                build.stdout
            } else {
                build.stderr
            }
        ));
    }
    let binary = repo.join("target/debug/skrzynka");
    if !binary.exists() {
        return Err(format!(
            "the build reported success but produced no {}",
            binary.display()
        ));
    }
    let output = Command::new(&binary)
        .arg("--version")
        .output()
        .map_err(|error| format!("cannot start {}: {error}", binary.display()))?;
    let version = common::Output {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    if !version.status.success() {
        return Err(format!(
            "{} could not report its version: {}",
            binary.display(),
            if version.stderr.is_empty() {
                &version.stdout
            } else {
                &version.stderr
            }
        ));
    }
    let pattern = regex::Regex::new(r"^skrzynka \d+\.\d+\.\d+").unwrap();
    if !pattern.is_match(&version.stdout) {
        return Err(format!("unexpected --version output: {:?}", version.stdout));
    }
    Ok(())
}
