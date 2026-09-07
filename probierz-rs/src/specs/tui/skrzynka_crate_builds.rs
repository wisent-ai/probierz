use crate::specs::{self, tui::common};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

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
    let build = common::run(
        "cargo",
        &common::strings(&["build", "--locked", "--all-targets"]),
        Some(&repo),
        &BTreeMap::new(),
        &[],
        None,
        Duration::from_secs(900),
    )?;
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
    let version = common::run(
        binary.to_string_lossy().as_ref(),
        &common::strings(&["--version"]),
        None,
        &BTreeMap::new(),
        &[],
        None,
        Duration::from_secs(30),
    )?;
    if !version.status.success() {
        return Err(format!(
            "{} could not report its version: {}",
            binary.display(),
            version.combined()
        ));
    }
    let pattern = regex::Regex::new(r"^skrzynka \d+\.\d+\.\d+").unwrap();
    if !pattern.is_match(&version.stdout) {
        return Err(format!("unexpected --version output: {:?}", version.stdout));
    }
    Ok(())
}
