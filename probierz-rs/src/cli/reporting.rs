//! Projections over evidence, the incident register, the gates, and execution.
//!
//! Moved verbatim out of `main.rs`, with one addition: `Incident`, the
//! register of failed attempts. It sits beside `Failures` because they answer
//! the same question at two ranges — what arrived from a running application,
//! and what a person or an agent recorded as an attempt that did not hold.

use clap::Subcommand;

use crate::{gate, incidents, run_flags_help};

#[derive(Debug, Subcommand)]
pub enum ReportingCommand {
    // PortStatus: status/history/dashboard/overview/intake
    /// Stability by run, journey, and test.
    History {
        app_id: Option<String>,
        target: Option<String>,
        #[arg(long, default_value_t = 50, value_parser = positive_history_limit)]
        limit: usize,
    },
    /// Product/version/journey evidence projection.
    Dashboard {
        app_id: String,
        limit: Option<String>,
    },
    /// Journey coverage, freshness against HEAD, and merge eligibility.
    Status {
        app_id: String,
        #[arg(long, default_value = "origin/main")]
        base: String,
        #[arg(long)]
        text: bool,
    },
    /// Unified app status, repository violations, and Stado fleet health.
    Overview {
        app_ids: Vec<String>,
        #[arg(long)]
        text: bool,
    },
    /// Fast all-app failure view without repository violation scans.
    Errors {
        app_ids: Vec<String>,
        #[arg(long)]
        text: bool,
    },
    /// Receive failure envelopes from desktop applications.
    Intake {
        #[command(subcommand)]
        command: IntakeCommand,
    },
    /// Counts and newest envelopes in the failure intake store.
    Failures {
        #[arg(long)]
        service: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// The register of attempts that did not hold: record, read, resolve.
    #[command(after_help = incidents::HELP)]
    Incident {
        #[command(subcommand)]
        command: incidents::IncidentCommand,
    },
    // PortGate: merge and release gates
    /// Gate configuration and activation status for an application.
    GateStatus { app_id: String },
    /// Judge the changes being pushed to a repository.
    GatePrepush {
        #[command(flatten)]
        args: gate::PrepushArgs,
    },
    /// Install the repository pre-push gate, preserving an existing hook.
    GateInstall {
        #[command(flatten)]
        args: gate::InstallArgs,
    },
    /// Evaluate evidence against a merge or release policy.
    GateEvaluate {
        #[command(flatten)]
        args: gate::GateArgs,
    },
    /// Enforce an activated merge or release policy.
    GateEnforce {
        #[command(flatten)]
        args: gate::GateArgs,
    },
    /// Require a green evaluation and activate its gate.
    GateActivate {
        #[command(flatten)]
        args: gate::GateArgs,
    },
    // PortRuns: execution, analysis, and matrix
    /// Is the target toolchain ready?
    Check { target: String },
    /// Install the browser or driver layers Probierz owns.
    #[command(after_help = crate::run::RUN_FLAGS_HELP)]
    Setup {
        target: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Execute a target, capture its artifacts, and analyze its report.
    #[command(after_help = crate::run::RUN_FLAGS_HELP)]
    Run {
        target: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Normalize a Playwright, WDIO, or canonical Probierz report.
    #[command(after_help = crate::run::RUN_FLAGS_HELP)]
    Analyze {
        report: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Select the targets and application journeys affected by changed files.
    #[command(after_help = crate::run::RUN_FLAGS_HELP)]
    Affected {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run every affected and ready target.
    #[command(after_help = crate::run::RUN_FLAGS_HELP)]
    Ci {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Plan or execute a declared application matrix.
    #[command(after_help = concat!(crate::matrix_flags_help!(), "\n\n", run_flags_help!()))]
    Matrix {
        app_id: String,
        profile: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

// PortStatus: status/history/dashboard/overview/intake
#[derive(Debug, Subcommand)]
pub enum IntakeCommand {
    /// Listen for wisent-errors envelopes.
    Serve {
        #[arg(long, default_value = "127.0.0.1:9790")]
        bind: String,
    },
}

pub fn positive_history_limit(value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| "--limit needs a positive number".to_string())?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return Err("--limit needs a positive number".to_string());
    }
    Ok((parsed.floor() as usize).max(1))
}

pub fn dashboard_limit(value: Option<&str>) -> usize {
    value
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value != 0.0)
        .map(|value| value.max(1.0).floor() as usize)
        .unwrap_or(500)
}
