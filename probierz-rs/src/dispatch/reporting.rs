//! Projections, the incident register, the gates and execution, dispatched
//! exactly as `main.rs` dispatched them.

use std::path::Path;

use crate::cli::reporting::{dashboard_limit, IntakeCommand, ReportingCommand};
use crate::failure::Answer;
use crate::{adoption, gate, incidents, run, status};

pub fn dispatch(harness: &Path, command: ReportingCommand) -> Answer {
    match command {
        // PortStatus: status/history/dashboard/overview/intake
        ReportingCommand::History {
            app_id,
            target,
            limit,
        } => status::history(
            harness,
            app_id.as_deref().unwrap_or("probierz"),
            target.as_deref(),
            limit,
        ),
        ReportingCommand::Dashboard { app_id, limit } => {
            status::dashboard(harness, &app_id, dashboard_limit(limit.as_deref()))
        }
        ReportingCommand::Status { app_id, base, text } => {
            let eligible = status::status(harness, &app_id, &base, text)?;
            if !eligible {
                std::process::exit(1);
            }
            Ok(())
        }
        ReportingCommand::Overview { app_ids, text } => {
            status::overview(harness, &app_ids, text, true)
        }
        ReportingCommand::Errors { app_ids, text } => {
            status::overview(harness, &app_ids, text, false)
        }
        ReportingCommand::Intake { command } => match command {
            IntakeCommand::Serve { bind } => status::intake_serve(Some(&bind)),
        },
        ReportingCommand::Failures {
            service,
            limit,
            json,
        } => status::failures(service.as_deref(), limit, json),
        ReportingCommand::Incident { command } => incidents::dispatch(harness, command),
        // PortGate: merge and release gates
        ReportingCommand::GateStatus { app_id } => gate::status(harness, &app_id),
        ReportingCommand::GatePrepush { args } => gate::prepush(harness, &args),
        ReportingCommand::GateInstall { args } => gate::install(harness, &args),
        ReportingCommand::GateEvaluate { args } => gate::evaluate(harness, &args),
        ReportingCommand::GateEnforce { args } => gate::enforce(harness, &args),
        ReportingCommand::GateActivate { args } => gate::activate(harness, &args),
        // PortRuns: execution, analysis, and matrix
        ReportingCommand::Check { target } => run::check(harness, &target),
        ReportingCommand::Setup { target, args } => run::setup(harness, &target, &args),
        ReportingCommand::Run { target, args } => {
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
