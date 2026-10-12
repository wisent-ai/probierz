//! Projections, the incident register, the gates and execution, dispatched
//! exactly as `main.rs` dispatched them.

use std::path::Path;

use crate::cli::reporting::{dashboard_limit, GateCommand, IntakeCommand, ReportingCommand};
use crate::failure::Answer;
use crate::{adoption, gate, incidents, run, status};

pub fn dispatch(harness: &Path, command: ReportingCommand) -> Answer {
    match command {
        // PortStatus: status/history/dashboard/overview/intake
        ReportingCommand::History {
            app_id,
            target,
            journey,
            limit,
        } => status::history(
            harness,
            &app_id,
            target.as_deref(),
            journey.as_deref(),
            limit.unwrap_or(usize::MAX),
        ),
        ReportingCommand::Dashboard { app_id, limit } => {
            status::dashboard(harness, &app_id, dashboard_limit(limit.as_deref()))
        }
        ReportingCommand::Status { app_id, base, json } => {
            status::status(harness, &app_id, base.as_deref(), !json)
        }
        ReportingCommand::Overview { app_ids, json } => {
            status::overview(harness, &app_ids, !json, true)
        }
        ReportingCommand::Errors { app_ids, json } => {
            status::overview(harness, &app_ids, !json, false)
        }
        ReportingCommand::Intake { command } => match command {
            IntakeCommand::Serve { bind } => status::intake_serve(&bind),
        },
        ReportingCommand::Failures {
            service,
            limit,
            json,
        } => status::failures(service.as_deref(), limit.unwrap_or(usize::MAX), json),
        ReportingCommand::Incident { command } => incidents::dispatch(harness, command),
        ReportingCommand::Benchmark { command } => crate::benchmark::dispatch(harness, command),
        // PortGate: merge and release gates
        ReportingCommand::Gate { command } => match command {
            GateCommand::Status { app_id } => gate::status(harness, &app_id),
            GateCommand::Prepush { args } => gate::prepush(harness, &args),
            GateCommand::Install { args } => gate::install(harness, &args),
            GateCommand::Uninstall { args } => gate::uninstall(harness, &args),
            GateCommand::Evaluate { args } => gate::evaluate(harness, &args),
            GateCommand::Enforce { args } => gate::enforce(harness, &args),
            GateCommand::Activate { args } => gate::activate(harness, &args),
            GateCommand::Deactivate { args } => gate::deactivate(harness, &args),
        },
        // PortRuns: execution, analysis, and matrix
        ReportingCommand::Check { target } => run::check(&target),
        ReportingCommand::Setup { target, args } => run::setup(harness, &target, &args),
        ReportingCommand::Run { target, args } => {
            // A product's first-use journey is played, not an npm target; it
            // takes nothing but the product its target names.
            if let Some(app_id) = target.strip_prefix(crate::firstuse::TARGET_PREFIX) {
                if let Some(extra) = args.first() {
                    return Err(crate::failure::Failure::invalid(
                        "first-use.run",
                        format!(
                            "probierz run {target} takes no further arguments; {extra:?} was given"
                        ),
                    ));
                }
                return crate::firstuse::run(harness, app_id);
            }
            let answer = run::run(harness, &target, &args);
            if answer.is_ok() {
                adoption::record_passing_quality_evidence_written();
            }
            answer
        }
        ReportingCommand::Analyze { report, args } => run::analyze(harness, &report, &args),
        ReportingCommand::Affected { args } => run::affected(harness, &args),
        ReportingCommand::Ci { args } => run::ci(harness, &args),
        ReportingCommand::Matrix {
            app_id,
            profile,
            args,
        } => run::matrix(harness, &app_id, &profile, &args),
    }
}
