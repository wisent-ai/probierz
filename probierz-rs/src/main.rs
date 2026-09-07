//! Probierz: journeys run where the product lives, and every run becomes
//! evidence.
//!
//! This binary is the product. The harness it reads — application manifests,
//! specs on disk, evidence under `test-results` — lives in the repository root
//! above this crate, so a checkout and an installed binary see the same
//! declarations.

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
// PortGate: merge and release gates
mod gate;
// PortRuns: execution, analysis, and matrix
mod run;
// PortEvidence: durable evidence, signing, publication, and retention
mod evidence;
// PortStado: remote Stado bridge
mod stado;

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use failure::{Answer, Failure};

#[derive(Debug, Parser)]
#[command(
    name = "probierz",
    about = "Proof that your software works as intended",
    version,
    disable_help_subcommand = true
)]
struct Cli {
    /// The harness root holding `apps/`, `packages/` and `test-results/`.
    /// Defaults to the repository this binary was built in, then the working
    /// directory.
    #[arg(long, global = true, value_name = "DIR")]
    harness: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    // Restored: adoption and local API
    /// Show the first-run journey and optionally adopt existing definitions.
    #[command(
        after_help = adoption::ONBOARDING_HELP,
        override_usage = "probierz onboarding [--reset] [--source <repository>] [--replace] [--json]"
    )]
    Onboarding {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        args: Vec<String>,
    },
    /// Durable project-adoption operations.
    #[command(after_help = adoption::PROJECT_HELP)]
    Project {
        #[command(subcommand)]
        command: adoption::ProjectCommand,
    },
    /// Run the loopback API used by Probierz Desktop.
    #[command(
        after_help = serve::HELP,
        override_usage = "probierz serve [--port N]"
    )]
    Serve {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        args: Vec<String>,
    },
    /// Every test surface, its tool, its npm script and its target coordinates.
    List,
    /// Registered products, their targets and their journeys.
    Apps,
    /// One validated product manifest.
    App { app_id: String },
    /// Run one built-in application setup, broker, or evaluation capability.
    #[command(after_help = apphook_help!())]
    Apphook {
        capability: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// The spec files on disk, optionally for one surface.
    Specs { surface: Option<String> },
    /// The static outline of one spec file: its describe and it titles.
    Describe { spec: String },
    /// The exact shell command that runs a target, printed and not run.
    Cmd { target: String },
    /// The run hosts this harness can use: local and the Stado providers.
    Hosts,
    // PortAuthoring: authoring, evaluation, and identity
    /// Exact path-independent harness and application source identity.
    SourceIdentity { app_id: String },
    /// Validate stable identifiers and native selectors.
    Accessibility { app_id: String },
    /// Draft, execute, and accept one real journey specification.
    AuthorSpec {
        app_id: String,
        journey: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        desc: String,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        app_path: Option<String>,
        #[arg(long = "paths")]
        mapping_paths: Vec<String>,
        #[arg(long, default_value_t = 3)]
        rounds: u32,
        #[arg(long)]
        dry_run: bool,
    },
    /// Draft and validate a complete application manifest.
    AuthorManifest {
        app_id: String,
        #[arg(long)]
        desc: String,
        #[arg(long)]
        target: String,
        #[arg(long = "repo", required = true)]
        repositories: Vec<String>,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        app_path: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long = "specs")]
        with_specs: bool,
    },
    /// Dispatch a bounded repair worker for a recorded failed run.
    Repair {
        app_id: String,
        #[arg(long = "run")]
        run_id: Option<String>,
        #[arg(long, default_value_t = 2)]
        rounds: u32,
        #[arg(long)]
        dry_run: bool,
    },
    /// Render and rubric-score a scientific figure pair.
    FigureEvaluate {
        #[arg(long)]
        reference: PathBuf,
        #[arg(long)]
        candidate: PathBuf,
        #[arg(long)]
        rubric: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long = "out")]
        output: Option<PathBuf>,
        #[arg(long)]
        router_url: Option<String>,
        #[arg(long)]
        tex_preamble: Option<PathBuf>,
        #[arg(long)]
        agent_id: Option<String>,
        #[arg(long)]
        router_token_stdin: bool,
    },
    /// Crawl and evaluate a declared SEO contract.
    SeoEvaluate {
        #[arg(long = "app", default_value = "landing-page")]
        app_id: String,
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        policy: Option<PathBuf>,
        #[arg(long)]
        brief: Option<PathBuf>,
        #[arg(long, default_value = "release")]
        mode: String,
        #[arg(long = "out")]
        output: Option<PathBuf>,
        #[arg(long)]
        production_evidence: Option<PathBuf>,
        #[arg(long)]
        primary_model: Option<String>,
        #[arg(long)]
        secondary_model: Option<String>,
        #[arg(long)]
        adjudicator_model: Option<String>,
        #[arg(long)]
        router_url: Option<String>,
        #[arg(long)]
        agent_id: Option<String>,
        #[arg(long)]
        private_key_file: Option<PathBuf>,
        #[arg(long)]
        router_token_stdin: bool,
    },
    // ReadmeGif
    /// Render a bounded, silent journey video as a looping README GIF.
    ReadmeGif {
        input: PathBuf,
        #[arg(long = "out")]
        output: PathBuf,
        #[arg(long, default_value_t = 0.0)]
        start: f64,
        #[arg(long, default_value_t = 12.0)]
        duration: f64,
        #[arg(long, default_value_t = 12.0)]
        fps: f64,
        #[arg(long, default_value_t = 960.0)]
        width: f64,
        #[arg(long)]
        force: bool,
    },
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
    #[command(after_help = run::RUN_FLAGS_HELP)]
    Setup {
        target: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Execute a target, capture its artifacts, and analyze its report.
    #[command(after_help = run::RUN_FLAGS_HELP)]
    Run {
        target: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Normalize a Playwright, WDIO, or canonical Probierz report.
    #[command(after_help = run::RUN_FLAGS_HELP)]
    Analyze {
        report: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Select the targets and application journeys affected by changed files.
    #[command(after_help = run::RUN_FLAGS_HELP)]
    Affected {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run every affected and ready target.
    #[command(after_help = run::RUN_FLAGS_HELP)]
    Ci {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Plan or execute a declared application matrix.
    #[command(after_help = concat!(matrix_flags_help!(), "\n\n", run_flags_help!()))]
    Matrix {
        app_id: String,
        profile: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    // PortEvidence: durable evidence, signing, publication, and retention
    /// Encrypt and authenticate one run's evidence artifacts.
    Protect {
        app_id: Option<String>,
        run_id: Option<String>,
        kind: Option<String>,
        #[arg(long)]
        key_file: Option<PathBuf>,
        #[arg(long = "remove-source")]
        remove_source: bool,
    },
    /// Authenticate and restore an encrypted evidence bundle.
    Restore {
        bundle: Option<PathBuf>,
        destination: Option<PathBuf>,
        #[arg(long)]
        key_file: Option<PathBuf>,
    },
    /// Plan or apply application evidence retention.
    Retention {
        app_id: Option<String>,
        #[arg(long)]
        at: Option<String>,
        #[arg(long)]
        apply: bool,
    },
    /// Find credentials and tokens in an evidence directory.
    SecretScan { directory: Option<PathBuf> },
    /// Query the tamper-evident access audit.
    Audit {
        app_id: Option<String>,
        #[arg(long = "run")]
        run_id: Option<String>,
        #[arg(long)]
        action: Option<String>,
        #[arg(long, default_value = "200")]
        limit: String,
    },
    /// Compare two recorded runs.
    Compare {
        left_run_id: Option<String>,
        right_run_id: Option<String>,
        app_id: Option<String>,
    },
    /// Find the newest passing run.
    LastGreen {
        app_id: Option<String>,
        target: Option<String>,
        journey: Option<String>,
    },
    /// Sign exact runs and policy into an evidence receipt.
    Receipt {
        app_id: Option<String>,
        release: Option<String>,
        expected_harness_sha: Option<String>,
        #[arg(long = "source-sha")]
        expected_source_sha: Option<String>,
        #[arg(long)]
        runs: Option<String>,
        #[arg(long)]
        journeys: Option<String>,
        #[arg(long, default_value = "E3")]
        minimum: String,
    },
    /// Verify a receipt signature, payload hash, and trust anchor.
    VerifyReceipt {
        file: Option<PathBuf>,
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        fingerprint: Option<String>,
    },
    /// Emit a verified immutable first-use publication manifest.
    Publication {
        receipt: Option<PathBuf>,
        attempt_id: Option<String>,
        journey_id: Option<String>,
        #[arg(long)]
        assets: Option<PathBuf>,
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        fingerprint: Option<String>,
    },
    /// Emit an Echo-ingestible onboarding proof manifest.
    PublishOnboarding {
        receipt: Option<PathBuf>,
        #[arg(long = "run")]
        run_id: Option<String>,
        #[arg(long = "journey")]
        journey_id: Option<String>,
        #[arg(long = "journey-version")]
        journey_version: Option<String>,
        #[arg(long = "journey-version-id")]
        journey_version_id: Option<String>,
        #[arg(long = "first-success-fact")]
        first_success_fact: Option<String>,
        #[arg(long = "screen")]
        screen_id: Option<String>,
        #[arg(long)]
        assets: Option<PathBuf>,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        fingerprint: Option<String>,
    },
    // PortStado: remote Stado bridge
    /// Submit, recover, resume, cancel, or author work on the Stado fleet.
    Stado {
        #[command(subcommand)]
        command: stado::StadoCommand,
    },
}

// PortStatus: status/history/dashboard/overview/intake
#[derive(Debug, Subcommand)]
enum IntakeCommand {
    /// Listen for wisent-errors envelopes.
    Serve {
        #[arg(long, default_value = "127.0.0.1:9790")]
        bind: String,
    },
}

fn positive_history_limit(value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| "--limit needs a positive number".to_string())?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return Err("--limit needs a positive number".to_string());
    }
    Ok((parsed.floor() as usize).max(1))
}

fn dashboard_limit(value: Option<&str>) -> usize {
    value
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value != 0.0)
        .map(|value| value.max(1.0).floor() as usize)
        .unwrap_or(500)
}

fn main() {
    let cli = Cli::parse();
    let harness = match resolve_harness(cli.harness.as_deref()) {
        Ok(root) => root,
        Err(failure) => std::process::exit(failure.report()),
    };
    let answer = dispatch(&harness, cli.command);
    if let Err(failure) = answer {
        std::process::exit(failure.report());
    }
}

fn dispatch(harness: &Path, command: Command) -> Answer {
    match command {
        Command::Onboarding { args } => adoption::onboarding(harness, &args),
        Command::Project { command } => adoption::dispatch(harness, command),
        Command::Serve { args } => serve::serve(harness, &args),
        Command::List => discovery::list(harness),
        Command::Apps => discovery::apps(harness),
        Command::App { app_id } => discovery::app(harness, &app_id),
        Command::Apphook { capability, args } => apphooks::command(harness, &capability, &args),
        Command::Specs { surface } => discovery::specs(harness, surface.as_deref()),
        Command::Describe { spec } => discovery::describe(harness, &spec),
        Command::Cmd { target } => discovery::cmd(harness, &target),
        Command::Hosts => discovery::hosts(),
        // PortStado: remote Stado bridge
        Command::Stado { command } => stado::dispatch(harness, command),
        // PortAuthoring: authoring, evaluation, and identity
        Command::SourceIdentity { app_id } => authoring::source_identity_command(harness, &app_id),
        Command::Accessibility { app_id } => {
            if !authoring::accessibility_command(harness, &app_id)? {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::AuthorSpec {
            app_id,
            journey,
            target,
            desc,
            base_url,
            app_path,
            mapping_paths,
            rounds,
            dry_run,
        } => {
            let result = authoring::author_spec(
                harness,
                &app_id,
                &journey,
                &target,
                &desc,
                base_url.as_deref(),
                app_path.as_deref(),
                &mapping_paths,
                rounds,
                dry_run,
            )?;
            if !authoring::print_result(result)? {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::AuthorManifest {
            app_id,
            desc,
            target,
            repositories,
            owner,
            base_url,
            app_path,
            dry_run,
            with_specs,
        } => {
            let result = authoring::author_manifest(
                harness,
                &app_id,
                &desc,
                owner.as_deref(),
                &repositories,
                &target,
                base_url.as_deref(),
                app_path.as_deref(),
                dry_run,
                with_specs,
            )?;
            if !authoring::print_result(result)? {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Repair {
            app_id,
            run_id,
            rounds,
            dry_run,
        } => {
            let result =
                authoring::repair_failed_run(harness, &app_id, run_id.as_deref(), rounds, dry_run)?;
            if !authoring::print_result(result)? {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::FigureEvaluate {
            reference,
            candidate,
            rubric,
            model,
            output,
            router_url,
            tex_preamble,
            agent_id,
            router_token_stdin,
        } => {
            let mut stdin = String::new();
            if router_token_stdin {
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin)?;
            }
            let mut lines = stdin.lines();
            let bearer = lines.next();
            let secret = lines.next();
            let result = authoring::evaluate_figure(
                harness,
                &reference,
                &candidate,
                rubric.as_deref(),
                output.as_deref(),
                model.as_deref(),
                router_url.as_deref(),
                tex_preamble.as_deref(),
                bearer,
                agent_id.as_deref(),
                secret,
            )?;
            if !authoring::print_result(result)? {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::SeoEvaluate {
            app_id,
            base_url,
            policy,
            brief,
            mode,
            output,
            production_evidence,
            primary_model,
            secondary_model,
            adjudicator_model,
            router_url,
            agent_id,
            private_key_file,
            router_token_stdin,
        } => {
            let mut stdin = String::new();
            if router_token_stdin {
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin)?;
            }
            let mut lines = stdin.lines();
            let bearer = lines.next();
            let secret = lines.next();
            let private_key = if router_token_stdin {
                Some(lines.collect::<Vec<_>>().join("\n"))
            } else {
                None
            };
            let result = authoring::evaluate_seo(
                harness,
                &app_id,
                &base_url,
                policy.as_deref(),
                brief.as_deref(),
                &mode,
                output.as_deref(),
                production_evidence.as_deref(),
                primary_model.as_deref(),
                secondary_model.as_deref(),
                adjudicator_model.as_deref(),
                router_url.as_deref(),
                agent_id.as_deref(),
                private_key_file.as_deref(),
                bearer,
                secret,
                private_key
                    .as_deref()
                    .filter(|value| !value.trim().is_empty()),
            )?;
            if !authoring::print_result(result)? {
                std::process::exit(1);
            }
            Ok(())
        }
        // ReadmeGif
        Command::ReadmeGif {
            input,
            output,
            start,
            duration,
            fps,
            width,
            force,
        } => readme_gif::create(readme_gif::Options {
            input,
            output,
            start_seconds: start,
            duration_seconds: duration,
            frames_per_second: fps,
            width,
            force,
        }),
        // PortStatus: status/history/dashboard/overview/intake
        Command::History {
            app_id,
            target,
            limit,
        } => status::history(
            harness,
            app_id.as_deref().unwrap_or("probierz"),
            target.as_deref(),
            limit,
        ),
        Command::Dashboard { app_id, limit } => {
            status::dashboard(harness, &app_id, dashboard_limit(limit.as_deref()))
        }
        Command::Status { app_id, base, text } => {
            let eligible = status::status(harness, &app_id, &base, text)?;
            if !eligible {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Overview { app_ids, text } => status::overview(harness, &app_ids, text, true),
        Command::Errors { app_ids, text } => status::overview(harness, &app_ids, text, false),
        Command::Intake { command } => match command {
            IntakeCommand::Serve { bind } => status::intake_serve(Some(&bind)),
        },
        Command::Failures {
            service,
            limit,
            json,
        } => status::failures(service.as_deref(), limit, json),
        // PortGate: merge and release gates
        Command::GateStatus { app_id } => gate::status(harness, &app_id),
        Command::GatePrepush { args } => gate::prepush(harness, &args),
        Command::GateInstall { args } => gate::install(harness, &args),
        Command::GateEvaluate { args } => gate::evaluate(harness, &args),
        Command::GateEnforce { args } => gate::enforce(harness, &args),
        Command::GateActivate { args } => gate::activate(harness, &args),
        // PortRuns: execution, analysis, and matrix
        Command::Check { target } => run::check(harness, &target),
        Command::Setup { target, args } => run::setup(harness, &target, &args),
        Command::Run { target, args } => {
            let answer = run::run(harness, &target, &args);
            if answer.is_ok() {
                adoption::record_passing_quality_evidence_written();
            }
            answer
        }
        Command::Analyze { report, args } => run::analyze(harness, &report, &args),
        Command::Affected { args } => run::affected(harness, &args),
        Command::Ci { args } => run::ci(harness, &args),
        Command::Matrix {
            app_id,
            profile,
            args,
        } => run::matrix(harness, &app_id, &profile, &args),
        // PortEvidence: durable evidence, signing, publication, and retention
        Command::Protect {
            app_id,
            run_id,
            kind,
            key_file,
            remove_source,
        } => evidence::protect(
            harness,
            app_id.as_deref(),
            run_id.as_deref(),
            kind.as_deref(),
            key_file.as_deref(),
            remove_source,
        ),
        Command::Restore {
            bundle,
            destination,
            key_file,
        } => evidence::restore(
            harness,
            bundle.as_deref(),
            destination.as_deref(),
            key_file.as_deref(),
        ),
        Command::Retention { app_id, at, apply } => {
            evidence::retention(harness, app_id.as_deref(), at.as_deref(), apply)
        }
        Command::SecretScan { directory } => evidence::secret_scan(directory.as_deref()),
        Command::Audit {
            app_id,
            run_id,
            action,
            limit,
        } => evidence::audit(
            harness,
            app_id.as_deref(),
            run_id.as_deref(),
            action.as_deref(),
            &limit,
        ),
        Command::Compare {
            left_run_id,
            right_run_id,
            app_id,
        } => evidence::compare(
            harness,
            left_run_id.as_deref(),
            right_run_id.as_deref(),
            app_id.as_deref(),
        ),
        Command::LastGreen {
            app_id,
            target,
            journey,
        } => evidence::last_green(
            harness,
            app_id.as_deref(),
            target.as_deref(),
            journey.as_deref(),
        ),
        Command::Receipt {
            app_id,
            release,
            expected_harness_sha,
            expected_source_sha,
            runs,
            journeys,
            minimum,
        } => evidence::receipt(
            harness,
            app_id.as_deref(),
            release.as_deref(),
            expected_harness_sha.as_deref(),
            expected_source_sha.as_deref(),
            runs.as_deref(),
            journeys.as_deref(),
            &minimum,
        ),
        Command::VerifyReceipt {
            file,
            public_key,
            fingerprint,
        } => evidence::verify_receipt(
            file.as_deref(),
            public_key.as_deref(),
            fingerprint.as_deref(),
        ),
        Command::Publication {
            receipt,
            attempt_id,
            journey_id,
            assets,
            public_key,
            fingerprint,
        } => evidence::publication(
            harness,
            receipt.as_deref(),
            attempt_id.as_deref(),
            journey_id.as_deref(),
            assets.as_deref(),
            public_key.as_deref(),
            fingerprint.as_deref(),
        ),
        Command::PublishOnboarding {
            receipt,
            run_id,
            journey_id,
            journey_version,
            journey_version_id,
            first_success_fact,
            screen_id,
            assets,
            output,
            public_key,
            fingerprint,
        } => evidence::publish_onboarding(
            receipt.as_deref(),
            run_id.as_deref(),
            journey_id.as_deref(),
            journey_version.as_deref(),
            journey_version_id.as_deref(),
            first_success_fact.as_deref(),
            screen_id.as_deref(),
            assets.as_deref(),
            output.as_deref(),
            public_key.as_deref(),
            fingerprint.as_deref(),
        ),
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
