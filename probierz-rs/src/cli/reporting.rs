//! Projections over evidence, the incident register, the gates, and execution.
//!
//! Moved verbatim out of `main.rs`, with one addition: `Incident`, the
//! register of failed attempts. It sits beside `Failures` because they answer
//! the same question at two ranges — what arrived from a running application,
//! and what a person or an agent recorded as an attempt that did not hold.

use std::num::NonZeroUsize;

use clap::Subcommand;

use crate::{gate, incidents, run_flags_help};

#[derive(Debug, Subcommand)]
pub enum ReportingCommand {
    // PortStatus: status/history/dashboard/overview/intake
    /// Stability by run, journey, and test, with the newest passing run
    /// (`summary.lastGreenRun`) of the selection.
    History {
        /// Application whose runs are read; no application is assumed.
        app_id: String,
        /// Only runs on this target.
        #[arg(long)]
        target: Option<String>,
        /// Only runs that carried this journey.
        #[arg(long)]
        journey: Option<String>,
        /// Newest runs to read; every run when omitted.
        #[arg(long, value_parser = positive_history_limit)]
        limit: Option<usize>,
    },
    /// Product/version/journey evidence projection.
    Dashboard {
        app_id: String,
        /// Newest runs to read; every run when omitted.
        #[arg(long, value_parser = positive_history_limit)]
        limit: Option<usize>,
    },
    /// Journey coverage, freshness against a base ref, and merge eligibility.
    Status {
        app_id: String,
        /// The ref the checkout is compared with; the branch it tracks when omitted.
        #[arg(long)]
        base: Option<String>,
        /// Print the report as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Unified app status, repository violations, and Stado fleet health.
    Overview {
        app_ids: Vec<String>,
        /// Print the report as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Fast all-app failure view without repository violation scans.
    Errors {
        app_ids: Vec<String>,
        /// Print the report as JSON instead of text.
        #[arg(long)]
        json: bool,
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
        /// Newest envelopes to print; every envelope when omitted.
        #[arg(long, value_parser = positive_history_limit)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// The register of attempts that did not hold: record, read, resolve.
    #[command(after_help = incidents::HELP)]
    Incident {
        #[command(subcommand)]
        command: incidents::IncidentCommand,
    },
    /// Our product and its rivals on the same versioned cases: run, read,
    /// compare, and the standing of the newest run.
    #[command(after_help = crate::benchmark::HELP)]
    Benchmark {
        #[command(subcommand)]
        command: crate::benchmark::BenchmarkCommand,
    },
    // PortGate: merge and release gates
    /// The repository pre-push gate and the merge and release gates: status,
    /// prepush, install, uninstall, evaluate, enforce, activate, deactivate.
    Gate {
        #[command(subcommand)]
        command: GateCommand,
    },
    // PortRuns: execution, analysis, and matrix
    /// Is the target toolchain ready? `suites/<checkout>` lists the test
    /// suites a checkout's release manifest declares and its newest kept run.
    Check { target: String },
    /// Install the browser or driver layers Probierz owns.
    #[command(after_help = crate::run::RUN_FLAGS_HELP)]
    Setup {
        target: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Execute a target, capture its artifacts, and analyze its report.
    /// `suites/<checkout> [<gate>…]` runs a checkout's own declared test
    /// suites and keeps the run under test-results/.suites/<product>.
    #[command(after_help = crate::run::RUN_FLAGS_HELP)]
    Run {
        target: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Normalize a WDIO or canonical Probierz report.
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

// PortGate: merge and release gates
#[derive(Debug, Subcommand)]
pub enum GateCommand {
    /// Gate configuration and activation status for an application.
    Status { app_id: String },
    /// Judge the changes being pushed to a repository.
    Prepush {
        #[command(flatten)]
        args: gate::PrepushArgs,
    },
    /// Install the repository pre-push gate, preserving an existing hook.
    Install {
        #[command(flatten)]
        args: gate::InstallArgs,
    },
    /// Remove the pre-push gate `gate install` wrote and put back the hook it chained.
    Uninstall {
        #[command(flatten)]
        args: gate::InstallArgs,
    },
    /// Evaluate evidence against a merge or release policy.
    Evaluate {
        #[command(flatten)]
        args: gate::GateArgs,
    },
    /// Enforce an activated merge or release policy.
    Enforce {
        #[command(flatten)]
        args: gate::GateArgs,
    },
    /// Require a green evaluation and activate its gate.
    Activate {
        #[command(flatten)]
        args: gate::GateArgs,
    },
    /// Withdraw one activated gate mode, so its policy is no longer enforced.
    Deactivate {
        #[command(flatten)]
        args: gate::DeactivateArgs,
    },
}

// PortStatus: status/history/dashboard/overview/intake
#[derive(Debug, Subcommand)]
pub enum IntakeCommand {
    /// Listen for wisent-errors envelopes.
    Serve {
        /// host:port the intake listens on. No address is assumed: the
        /// service declaration that runs the intake names it.
        #[arg(long)]
        bind: String,
    },
}

/// A count of rows or repetitions: a whole number above zero. Anything else
/// is refused naming it, never rounded or clamped into one.
pub fn positive_history_limit(value: &str) -> Result<usize, String> {
    value
        .parse::<NonZeroUsize>()
        .map(NonZeroUsize::get)
        .map_err(|error| format!("needs a whole number above zero, not {value:?}: {error}"))
}

/// The rows a `--limit` reads: that many, or every one when it was omitted.
pub fn newest(limit: Option<usize>) -> usize {
    match limit {
        Some(limit) => limit,
        None => usize::MAX,
    }
}
