//! Probierz: journeys run where the product lives, and every run becomes
//! evidence.
//!
//! This binary is the product. The harness it reads — application manifests,
//! specs on disk, evidence under `test-results` — lives in the repository root
//! above this crate, so a checkout and an installed binary see the same
//! declarations.
//!
//! The command surface is in [`cli`] and its dispatch in [`dispatch`], one
//! file per domain. They used to be here, 932 lines of them, and a file that
//! long cannot be edited under this workspace's length limit — so the surface
//! could not be extended at all. The split is what made the incident register
//! addable; nothing about the commands themselves changed.

mod adoption;
mod apphooks;
mod discovery;
mod failure;
mod manifest;
mod serve;
// PortAuthoring: authoring, evaluation, and identity
mod authoring;
// ReadmeGif
mod cua;
mod readme_gif;
mod specs;
mod tui;
// PortStatus: status/history/dashboard/overview/intake
mod status;
// PortIncidents: the register of attempts that did not hold
mod incidents;
// PortGate: merge and release gates
mod gate;
// PortRuns: execution, analysis, and matrix
mod run;
// PortEvidence: durable evidence, signing, publication, and retention
mod evidence;
// PortStado: remote Stado bridge
mod stado;

mod cli;
mod dispatch;

use std::path::{Path, PathBuf};

use clap::Parser;

use failure::Failure;

fn main() {
    let parsed = cli::Cli::parse();
    let harness = match resolve_harness(parsed.harness.as_deref()) {
        Ok(root) => root,
        Err(failure) => std::process::exit(failure.report()),
    };
    let answer = dispatch::dispatch(&harness, parsed.command);
    if let Err(failure) = answer {
        std::process::exit(failure.report());
    }
}

/// Where the harness is. A binary built inside the repository knows its own
/// source root; an installed one is told, or falls back to the working
/// directory. A guess that lands on the wrong `apps/` would answer about
/// another product's journeys, so an unusable root is refused rather than
/// replaced.
fn resolve_harness(explicit: Option<&Path>) -> Result<PathBuf, Failure> {
    let candidates: Vec<PathBuf> = match explicit {
        Some(path) => vec![path.to_path_buf()],
        None => {
            let built_in = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map(Path::to_path_buf);
            let mut list = Vec::new();
            if let Ok(from_env) = std::env::var("PROBIERZ_HARNESS_DIR") {
                if !from_env.trim().is_empty() {
                    list.push(PathBuf::from(from_env));
                }
            }
            if let Some(root) = built_in {
                list.push(root);
            }
            list.push(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            list
        }
    };
    for candidate in &candidates {
        if candidate.join("apps").is_dir() {
            return Ok(candidate.clone());
        }
    }
    Err(Failure::config(
        "harness.resolve",
        format!(
            "no harness root with an apps/ directory among: {}",
            candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ))
}
