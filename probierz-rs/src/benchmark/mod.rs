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
mod autonomy;
mod help;
mod inputs;
mod measure;
mod record;
mod scout;

use std::path::Path;

use clap::Subcommand;

use crate::failure::Answer;
use record::{catalog, commands, pursue};

pub use help::HELP;

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
    /// Scout a new product from a Trends topic and the products it shows.
    Scout {
        topic: String,
        /// The GitHub owner the new product's repository is created under.
        #[arg(long)]
        owner: String,
        /// How many of the topic's newest observations the model reads.
        #[arg(long, default_value_t = 40)]
        observations: usize,
        /// Drafts allowed per question before the command refuses.
        #[arg(long, default_value_t = 3)]
        rounds: u32,
    },
    /// Create the scouted product through Stado and start its benchmark.
    Adopt {
        brief: std::path::PathBuf,
        /// The operator's authority to create the brief's private repositories.
        #[arg(long)]
        allow_create: bool,
        /// How many cases the first suite holds.
        #[arg(long, default_value_t = 5)]
        cases: usize,
        /// Suite drafts allowed before the command refuses.
        #[arg(long, default_value_t = 3)]
        rounds: u32,
    },
    /// One pass of the loop without the operator, under his written policy.
    Cycle {
        /// The policy; without it, autonomy.yaml in the harness root.
        #[arg(long)]
        policy: Option<std::path::PathBuf>,
    },
    /// Have Stado run the cycle on a cron, pinned to one host.
    Schedule {
        /// Five-field cron expression, in UTC.
        #[arg(long)]
        cron: String,
        /// The Stado host the cycle runs on.
        #[arg(long)]
        host: String,
        /// The Probierz harness directory on that host.
        #[arg(long)]
        harness_dir: String,
        /// A secret the cycle reads, as NAME=SKARBIEC_ITEM#FIELD; repeatable.
        #[arg(long = "secret-env")]
        secrets: Vec<String>,
        /// A setting the cycle reads that is not secret, as NAME=VALUE, such as
        /// STADO_MODEL_ROUTER_URL or PROBIERZ_MODEL_AGENT_ID; repeatable.
        #[arg(long = "env", value_name = "NAME=VALUE")]
        settings: Vec<String>,
        /// The policy file on that host; without it, the harness's autonomy.yaml.
        #[arg(long)]
        policy: Option<String>,
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
        BenchmarkCommand::Scout {
            topic,
            owner,
            observations,
            rounds,
        } => scout::scout(harness, &topic, &owner, observations, rounds),
        BenchmarkCommand::Adopt {
            brief,
            allow_create,
            cases,
            rounds,
        } => scout::adopt(harness, &brief, allow_create, cases, rounds),
        BenchmarkCommand::Cycle { policy } => autonomy::cycle(harness, policy.as_deref()),
        BenchmarkCommand::Schedule {
            cron,
            host,
            harness_dir,
            secrets,
            settings,
            policy,
        } => autonomy::schedule(&cron, &host, &harness_dir, &secrets, &settings, policy.as_deref()),
    }
}
