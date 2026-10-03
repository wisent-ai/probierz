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

mod author;
mod inputs;
mod measure;
mod record;

use std::path::Path;

use clap::Subcommand;

use crate::failure::Answer;
use record::{catalog, commands, pursue};

pub const HELP: &str = "\
A benchmark runs our product and its rivals on the same versioned cases and
records who passed, how fast, and at what cost.

  probierz benchmark suites <app>
  probierz benchmark run <app> --suite <id> [--contender <id>]... [--repetitions N]
  probierz benchmark list <app> [--suite <id>] [--limit N]
  probierz benchmark show <app> <run-id>
  probierz benchmark compare <app> --baseline <run-id> --candidate <run-id>
  probierz benchmark standing <app> --suite <id>
  probierz benchmark rivals <app>
  probierz benchmark roadmap <app> --suite <id>
  probierz benchmark pursue <app> --suite <id> --case <id> --budget-usd <USD>
  probierz benchmark author-suite <app> --suite <id> [--cases N] [--rounds N]
  probierz benchmark author <app> --contender <id> --suite <id> [--ours] [--rounds N]

The manifest declares `benchmark.suites.<id>: <suite.json>` and
`benchmark.contenders.<id>: {program, args, env, ours}`. A contender reads one
ai.wisent.probierz.benchmark.task.v1 document on stdin and writes one
ai.wisent.probierz.benchmark.result.v1 document on stdout. Its environment is
empty except for the variables its `env` names. A suite's `variables` map each
`${NAME}` placeholder in a case input to the variable Probierz fills it from.

The product catalog Stado serves names the product's rivals and the suites
that measure them. `rivals` refuses while a named rival has no contender or a
named suite is not declared. `roadmap` writes one catalog roadmap item per
case the newest run lost, and withdraws the item once ours wins that case.

Nothing in a benchmark is written by hand for one product. `author-suite`
drafts a suite from the catalog record of the product and its rivals through
the Stado model router, judges it and declares it; an existing suite file is
never overwritten. `author` drafts the driver of our contender (--ours) or of
a rival the catalog names, places it under benchmark/contenders/<id>/ or
benchmark/rivals/<id>/ in the product's tree, declares it, and verifies it
with a recorded run of that contender alone, redrafting while an attempt
breaks the contract or fails with an error. A declared driver is verified
first and redrafted only if it fails.

`pursue` hands one case the newest run lost to Jeden as a durable pursuit
request in our contender's checkout. Jeden's verdict does not close it:
Probierz then records a new run of the suite itself, accepts the case only
when ours wins or ties it in that run, and brings the roadmap in line with
that run. A pursuit that reports success while the case is still lost is
refused.";

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
    /// The catalog's rivals for this product against the declared contenders
    /// and suites; refuses while any rival or suite is unmeasured.
    Rivals { app_id: String },
    /// Write the newest run's lost cases into the product's catalog roadmap,
    /// and withdraw the items of cases ours now wins.
    Roadmap {
        app_id: String,
        #[arg(long)]
        suite: String,
    },
    /// Draft, judge, write and declare a suite for this product and its rivals.
    AuthorSuite {
        app_id: String,
        #[arg(long)]
        suite: String,
        /// How many cases the suite holds.
        #[arg(long, default_value_t = 5)]
        cases: usize,
        /// Drafts allowed before the command refuses.
        #[arg(long, default_value_t = 3)]
        rounds: u32,
    },
    /// Draft, place, declare and verify one contender's driver.
    Author {
        app_id: String,
        #[arg(long)]
        contender: String,
        #[arg(long)]
        suite: String,
        /// The contender is our own product rather than a catalog rival.
        #[arg(long)]
        ours: bool,
        /// Drafts allowed before the command refuses.
        #[arg(long, default_value_t = 3)]
        rounds: u32,
    },
    /// Hand one lost case to a Jeden pursuit, then decide it with a new run.
    Pursue {
        app_id: String,
        #[arg(long)]
        suite: String,
        /// The case the newest run of the suite lost.
        #[arg(long)]
        case: String,
        /// The most the pursuit may spend on models, in US dollars.
        #[arg(long)]
        budget_usd: String,
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
        BenchmarkCommand::Rivals { app_id } => catalog::rivals(harness, &app_id),
        BenchmarkCommand::Roadmap { app_id, suite } => catalog::roadmap(harness, &app_id, &suite),
        BenchmarkCommand::AuthorSuite {
            app_id,
            suite,
            cases,
            rounds,
        } => author::suite(harness, &app_id, &suite, cases, rounds),
        BenchmarkCommand::Author {
            app_id,
            contender,
            suite,
            ours,
            rounds,
        } => author::contender(harness, &app_id, &contender, &suite, ours, rounds),
        BenchmarkCommand::Pursue {
            app_id,
            suite,
            case,
            budget_usd,
        } => pursue::pursue(harness, &app_id, &suite, &case, &budget_usd),
    }
}
