//! The source a suite run is bound to: the checkout's revision and every
//! path whose content differs from it, read from Git itself.

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

use crate::failure::Failure;

/// What `git` answers about the checkout.
pub(crate) struct SourceIdentity {
    pub(crate) revision: String,
    /// The revision as `git rev-parse --short` abbreviates it.
    pub(crate) short_revision: String,
    pub(crate) dirty_paths: Vec<String>,
}

fn git(repo: &Path, args: &[&str]) -> Result<String, Failure> {
    let mut command = Command::new("git");
    command.args(args).current_dir(repo);
    let output = stado_wait::output(&mut command).map_err(|error| {
        Failure::unavailable(
            "suites.identity",
            format!(
                "git {} could not run in {}: {error}",
                args.join(" "),
                repo.display()
            ),
        )
    })?;
    if !output.status.success() {
        return Err(Failure::config(
            "suites.identity",
            format!(
                "git {} in {} ended with {}: {}",
                args.join(" "),
                repo.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// The revision and the dirty paths, each refused with Git's own words when
/// the checkout cannot answer.
pub(crate) fn read(repo: &Path) -> Result<SourceIdentity, Failure> {
    let revision = git(repo, &["rev-parse", "HEAD"])?.trim().to_string();
    if revision.is_empty() {
        return Err(Failure::config(
            "suites.identity",
            format!("git rev-parse HEAD in {} answered nothing", repo.display()),
        ));
    }
    let short_revision = git(repo, &["rev-parse", "--short", "HEAD"])?
        .trim()
        .to_string();
    let status = git(repo, &["status", "--porcelain", "--untracked-files=all"])?;
    let dirty_paths = status
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect();
    Ok(SourceIdentity {
        revision,
        short_revision,
        dirty_paths,
    })
}

impl SourceIdentity {
    pub(crate) fn document(&self, repo: &Path) -> Value {
        json!({
            "repository": repo,
            "revision": self.revision,
            "shortRevision": self.short_revision,
            "dirty": !self.dirty_paths.is_empty(),
            "dirtyPaths": self.dirty_paths,
        })
    }
}
