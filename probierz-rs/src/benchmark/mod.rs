//! Benchmarks: our product and its rivals on the same versioned cases.
//!
//! A journey answers "does this revision do what it promises". A benchmark
//! answers the question a release decision cannot answer without it: on the
//! same tasks, measured the same way, does our product do better than the
//! products it competes with, and where does it lose.
//!
//! Probierz owns the measurement, not the contenders. A product declares its
//! suites and contenders under `benchmark:` in its own manifest. A suite is a
//! versioned JSON file of cases with assertions; a contender is a program that
//! reads one task on stdin and writes one result on stdout, so every rival,
//! whatever its language or transport, is driven through the same contract.
//! A run is recorded under `test-results/.benchmark/<app>/` with the suite's
//! hash and every contender's source revision, and nothing in it is ever
//! rewritten.

mod inputs;
mod measure;
mod record;

use std::path::Path;

use clap::Subcommand;

use crate::failure::Answer;
use record::commands;

pub const HELP: &str = "\
A benchmark runs our product and its rivals on the same versioned cases and
records who passed, how fast, and at what cost.

  probierz benchmark suites <app>
  probierz benchmark run <app> --suite <id> [--contender <id>]... [--repetitions N]
  probierz benchmark list <app> [--suite <id>] [--limit N]
  probierz benchmark show <app> <run-id>
  probierz benchmark compare <app> --baseline <run-id> --candidate <run-id>
  probierz benchmark standing <app> --suite <id>

The manifest declares `benchmark.suites.<id>: <suite.json>` and
`benchmark.contenders.<id>: {program, args, env, ours}`. A contender reads one
ai.wisent.probierz.benchmark.task.v1 document on stdin and writes one
ai.wisent.probierz.benchmark.result.v1 document on stdout.";

pub(crate) const SUITE_SCHEMA: &str = "ai.wisent.probierz.benchmark.suite.v1";
pub(crate) const TASK_SCHEMA: &str = "ai.wisent.probierz.benchmark.task.v1";
pub(crate) const RESULT_SCHEMA: &str = "ai.wisent.probierz.benchmark.result.v1";
pub(crate) const RUN_SCHEMA: &str = "ai.wisent.probierz.benchmark.run.v1";

#[derive(Debug, Subcommand)]
pub enum BenchmarkCommand {
    /// Every suite and contender the product's manifest declares.
    Suites { app_id: String },
    /// Run every case of one suite against the chosen contenders and record it.
    Run {
        app_id: String,
        #[arg(long)]
        suite: String,
        /// A declared contender; repeat it to choose several. Without it every
        /// declared contender runs.
        #[arg(long = "contender")]
        contenders: Vec<String>,
        /// How many times each case runs per contender; overrides the suite.
        #[arg(long, value_parser = crate::cli::reporting::positive_history_limit)]
        repetitions: Option<usize>,
    },
    /// Recorded runs, newest first.
    List {
        app_id: String,
        #[arg(long)]
        suite: Option<String>,
        #[arg(long, default_value_t = 20, value_parser = crate::cli::reporting::positive_history_limit)]
        limit: usize,
    },
    /// One recorded run with every sample.
    Show { app_id: String, run_id: String },
    /// Two runs of the same suite, contender by contender.
    Compare {
        app_id: String,
        #[arg(long)]
        baseline: String,
        #[arg(long)]
        candidate: String,
    },
    /// The newest run of a suite, case by case: who won and where we lost.
    Standing {
        app_id: String,
        #[arg(long)]
        suite: String,
    },
}

pub fn dispatch(harness: &Path, command: BenchmarkCommand) -> Answer {
    match command {
        BenchmarkCommand::Suites { app_id } => commands::suites(harness, &app_id),
        BenchmarkCommand::Run {
            app_id,
            suite,
            contenders,
            repetitions,
        } => commands::run(harness, &app_id, &suite, &contenders, repetitions),
        BenchmarkCommand::List {
            app_id,
            suite,
            limit,
        } => commands::list(harness, &app_id, suite.as_deref(), limit),
        BenchmarkCommand::Show { app_id, run_id } => commands::show(harness, &app_id, &run_id),
        BenchmarkCommand::Compare {
            app_id,
            baseline,
            candidate,
        } => commands::compare(harness, &app_id, &baseline, &candidate),
        BenchmarkCommand::Standing { app_id, suite } => {
            commands::standing(harness, &app_id, &suite)
        }
    }
}
