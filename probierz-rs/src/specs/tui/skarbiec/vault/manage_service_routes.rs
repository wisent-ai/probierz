//! The capability-routes journey: a scratch vault, one login item, and every
//! routes command run against it, in phases that read top to bottom.

mod adds;
mod journey;
mod setup;
mod trace;
mod verify;

use std::fs;
use std::path::Path;
use std::process::Command;

use regex::Regex;

use crate::specs;
use crate::specs::tui::skarbiec::fixture;

use trace::{Outcome, Source};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = fixture::binary(context);
    let manifest = fs::read_to_string(context.harness.join("apps/skarbiec/probierz.yaml"))
        .map_err(|error| {
            format!("skarbiec manifest must provide the source repository root: {error}")
        })?;
    let source_root = manifest
        .lines()
        .find_map(|line| line.strip_prefix("  - root: "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "skarbiec manifest must provide the source repository root".to_string())?;
    let revision = Command::new("/usr/bin/git")
        .args(["-C", source_root, "rev-parse", "HEAD"])
        .output()
        .map_err(|error| format!("cannot resolve skarbiec source revision: {error}"))?;
    fixture::ensure(
        revision.status.success(),
        format!(
            "cannot resolve skarbiec source revision: {}",
            String::from_utf8_lossy(&revision.stderr)
        ),
    )?;
    let source_revision = String::from_utf8_lossy(&revision.stdout).trim().to_string();
    let sha = Regex::new(r"^[0-9a-f]{40}$").map_err(|error| error.to_string())?;
    fixture::ensure(
        sha.is_match(&source_revision),
        "skarbiec source revision is not a full Git SHA",
    )?;
    let status = Command::new("/usr/bin/git")
        .args(["-C", source_root, "status", "--porcelain"])
        .output()
        .map_err(|error| format!("cannot inspect skarbiec source state: {error}"))?;
    fixture::ensure(
        status.status.success(),
        format!(
            "cannot inspect skarbiec source state: {}",
            String::from_utf8_lossy(&status.stderr)
        ),
    )?;
    let source_dirty = !status.stdout.is_empty();

    let temp_dir = fixture::scratch("skb-routes")?;
    let result = run_fixture(
        context,
        &binary,
        source_root,
        &source_revision,
        source_dirty,
        &temp_dir,
    );
    fixture::clean(&temp_dir);
    result
}

fn run_fixture(
    context: &specs::Context,
    binary: &str,
    source_root: &str,
    source_revision: &str,
    source_dirty: bool,
    temp_dir: &Path,
) -> Result<(), String> {
    let (mut journey, routes_phase_start) = setup::start(context, binary, temp_dir)?;
    let backup = adds::add_routes(&mut journey)?;
    let sound_verify = verify::verify_sound(&mut journey)?;
    let broken_report = verify::verify_broken(&mut journey)?;
    trace::finish(
        journey,
        routes_phase_start,
        Source {
            root: source_root,
            revision: source_revision,
            dirty: source_dirty,
        },
        Outcome {
            sound_verify,
            broken_report,
            backup,
        },
    )
}
