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
mod json_file;
mod manifest;
mod paths;
// PortAuthoring: authoring, evaluation, and identity
mod authoring;
// ReadmeGif
mod cua;
mod readme_gif;
mod specs;
// PortStatus: status/history/dashboard/overview/intake
mod status;
// PortIncidents: the register of attempts that did not hold
mod incidents;
// Our product against its rivals on the same versioned cases
mod benchmark;
// PortGate: merge and release gates
mod gate;
// PortRuns: execution, analysis, and matrix
mod run;
// A product's first-use journey, played as a new user: `probierz run first-use/<app>`
mod firstuse;
// A product checkout's own declared test suites, run and kept: `probierz run suites/<checkout>`
mod suites;
// PortEvidence: durable evidence, signing, publication, and retention
mod evidence;
// PortStado: remote Stado bridge
mod stado;

mod capture;
mod cli;
mod dispatch;
// The stdio MCP server, `probierz mcp`: the same commands, run in-process.
mod mcp;

use std::path::{Path, PathBuf};

use clap::Parser;

use failure::Failure;

fn main() {
    let parsed = cli::Cli::parse();
    failure::answer_as_text(parsed.text);
    // The failure intake and its reader are host-level: they keep envelopes
    // under ~/.probierz and never read a harness, so a service started with no
    // project directory - launchd starts it in / - is not refused for lacking
    // one.
    let host_level = matches!(
        parsed.command,
        cli::Command::Reporting(
            cli::reporting::ReportingCommand::Intake { .. }
                | cli::reporting::ReportingCommand::Failures { .. }
        )
    );
    let harness = if host_level {
        PathBuf::new()
    } else {
        match resolve_harness(parsed.harness.as_deref()) {
            Ok(root) => root,
            Err(failure) => std::process::exit(failure.report()),
        }
    };
    let answer = dispatch::dispatch(&harness, parsed.command);
    if let Err(failure) = answer {
        std::process::exit(failure.report());
    }
}

/// Run one CLI invocation in this process and answer what it printed. The
/// MCP server calls this for every tool: `arguments` is the argv the tool
/// routes to, without the program name, and a refusal comes back as the
/// failure's structured line so the caller reads the same words a shell
/// would have read on stderr.
pub(crate) fn run_in_process(harness: &Path, arguments: &[String]) -> Result<String, String> {
    let argv: Vec<String> = std::iter::once("probierz".to_string())
        .chain(arguments.iter().cloned())
        .collect();
    let parsed = cli::Cli::try_parse_from(&argv).map_err(|error| error.to_string())?;
    let (answer, output) = capture::captured(|| dispatch::dispatch(harness, parsed.command));
    match answer {
        Ok(()) => Ok(output),
        Err(failure) => Err(format!(
            "{} at {}: {}",
            failure.code.as_str(),
            failure.point,
            failure.detail
        )),
    }
}

/// Where the harness is. A binary built inside the repository knows its own
/// source root; an installed one is told, or falls back to the working
/// directory. A guess that lands on the wrong `apps/` would answer about
/// another product's journeys, so an unusable root is refused rather than
/// replaced.
fn resolve_harness(explicit: Option<&Path>) -> Result<PathBuf, Failure> {
    let mut candidates: Vec<PathBuf> = match explicit {
        Some(path) => vec![path.to_path_buf()],
        None => {
            let built_in = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map(Path::to_path_buf);
            let mut list = Vec::new();
            match std::env::var("PROBIERZ_HARNESS_DIR") {
                Ok(from_env) if !from_env.trim().is_empty() => list.push(PathBuf::from(from_env)),
                Ok(_blank) => {}
                Err(std::env::VarError::NotPresent) => {}
                Err(error) => {
                    return Err(Failure::config(
                        "harness.resolve",
                        format!("PROBIERZ_HARNESS_DIR cannot be read: {error}"),
                    ))
                }
            }
            if let Some(root) = built_in {
                list.push(root);
            }
            list
        }
    };
    for candidate in &candidates {
        if holds_apps(candidate)? {
            return Ok(candidate.clone());
        }
    }
    // The working directory is the last candidate, read only when the others
    // hold no apps/; one that cannot be read is named in the refusal.
    let mut unreadable = String::new();
    if explicit.is_none() {
        match std::env::current_dir() {
            Ok(working) if holds_apps(&working)? => return Ok(working),
            Ok(working) => candidates.push(working),
            Err(error) => unreadable = format!("; the working directory cannot be read: {error}"),
        }
    }
    Err(Failure::config(
        "harness.resolve",
        format!(
            "no harness root with an apps/ directory among: {}{unreadable}",
            candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ))
}

/// Whether `root` carries an `apps/` directory. One whose `apps` cannot be
/// examined is refused naming it: read as absent, the search moved on to the
/// next candidate and answered about another checkout's journeys.
fn holds_apps(root: &Path) -> Result<bool, Failure> {
    let apps = root.join("apps");
    match std::fs::metadata(&apps) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Failure::config(
            "harness.resolve",
            format!("{} cannot be examined: {error}", apps.display()),
        )),
    }
}
