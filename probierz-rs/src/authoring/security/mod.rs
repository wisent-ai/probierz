//! Model-assisted checks of an application's explicitly owned HTTP scope.
mod journal;
mod model;
mod probe;
mod scope;

use crate::authoring::{app_source_identity, manifest, Failure};
pub use journal::inspect_security_journal;
use journal::Journal;
use model::{Model, Plan};
use probe::Probe;
use scope::Scope;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs::File, io::Write, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Advice {
    summary: String,
    recommendations: Vec<Recommendation>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Recommendation {
    check: String,
    explanation: String,
    remediation: String,
}

/// Keep source identity, authorized scope, model provenance, observations and failures together.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_security(
    harness: &Path,
    app: &str,
    output: &Path,
    model: Option<&str>,
    router_url: Option<&str>,
    bearer: Option<&str>,
    agent: Option<&str>,
    secret: Option<&str>,
) -> Result<Value, Failure> {
    let loaded = manifest::load(harness, app)?;
    let declaration = loaded.document.get("security").ok_or_else(|| Failure::config("evaluate.security.scope",
        "the application manifest must declare security ownership, authorization, origin, addresses, response_bytes and checks"))?;
    let scope: Scope = serde_yaml::from_value(declaration.clone()).map_err(|e| {
        Failure::config(
            "evaluate.security.scope",
            format!("invalid security declaration: {e}"),
        )
    })?;
    scope
        .validate()
        .map_err(|e| Failure::config("evaluate.security.scope", e))?;
    let owner = loaded
        .document
        .get("owner")
        .and_then(serde_yaml::Value::as_str)
        .ok_or_else(|| Failure::config("evaluate.security.scope", "manifest owner is required"))?;
    if scope.owner != owner {
        return Err(Failure::config(
            "evaluate.security.scope",
            "security owner must match the registered application's owner",
        ));
    }
    let identity = app_source_identity(harness, app, None)?;
    let mut report_file = File::create_new(output).map_err(|e| {
        Failure::config(
            "evaluate.security.report",
            format!("{}: {e}", output.display()),
        )
    })?;
    let mut observations = Vec::new();
    let mut plan = Value::Null;
    let mut analysis = Value::Null;
    let started = chrono::Utc::now();
    let mut journal = Journal::create(output, app, &identity);
    let mut outcome: Result<(), String> = (|| {
        let journal = journal.as_mut().map_err(|error| error.clone())?;
        journal.begin("resolve-scope", None)?;
        // Reject out-of-scope DNS before contacting either a target or a model.
        let probe = Probe::new(&scope)?;
        if model.is_some() {
            journal.begin("model-configuration", None)?;
        }
        let reviewer = model
            .map(|route| Model::new(route, router_url, bearer, agent, secret))
            .transpose()?;
        let selected = if let Some(reviewer) = &reviewer {
            journal.begin("model-plan", None)?;
            let declared: Vec<_> = scope
                .checks
                .iter()
                .map(|check| json!({"id": check.id(), "path": check.path()}))
                .collect();
            plan = reviewer.ask("You are Probierz's authorized defensive security reviewer. Return only JSON {\"checks\":[\"id\"]}. Prioritize every declared check exactly once. Do not invent checks, destinations or commands.", &json!({"checks": declared}))?;
            let selection: Plan = serde_json::from_value(plan["answer"].clone())
                .map_err(|e| format!("invalid model plan: {e}"))?;
            scope.selected(&selection.checks)?
        } else {
            scope.checks.iter().collect()
        };
        for check in selected {
            journal.begin("check", Some(check.id()))?;
            let observation = probe
                .check(&scope, check)
                .map_err(|error| format!("check {}: {error}", check.id()))?;
            let persisted = journal.observed(&observation);
            observations.push(observation);
            persisted?;
        }
        if let Some(reviewer) = &reviewer {
            journal.begin("model-advice", None)?;
            analysis = reviewer.ask("Explain these authorized security observations. Return only JSON {\"summary\":\"text\",\"recommendations\":[{\"check\":\"declared-id\",\"explanation\":\"text\",\"remediation\":\"text\"}]}. Do not claim untested vulnerabilities or complete security. Never suggest probing other systems. Advice is not executable; the deterministic observations are authoritative.", &json!({"observations": observations}))?;
            let advice: Advice = serde_json::from_value(analysis["answer"].clone())
                .map_err(|e| format!("invalid model advice: {e}"))?;
            if advice.summary.trim().is_empty() {
                return Err("model advice summary is empty".into());
            }
            for recommendation in &advice.recommendations {
                if !scope
                    .checks
                    .iter()
                    .any(|check| check.id() == recommendation.check)
                    || recommendation.explanation.trim().is_empty()
                    || recommendation.remediation.trim().is_empty()
                {
                    return Err("model recommendation must refer to a declared check and include explanation and remediation".into());
                }
            }
        }
        Ok(())
    })();
    if let Ok(journal) = &mut journal {
        if let Err(error) = journal.finish(outcome.as_ref().err().map(String::as_str)) {
            outcome = Err(match outcome {
                Ok(()) => error,
                Err(cause) => format!("{cause}; additionally, {error}"),
            });
        }
    }
    let complete = outcome.is_ok();
    let report = json!({"kind": "probierz-security-evaluation", "appId": app,
        "startedAt": started, "finishedAt": chrono::Utc::now(), "sourceIdentity": identity,
        "scope": scope, "modelRequested": model, "plan": plan, "analysis": analysis,
        "mode": if model.is_some() { "model-assisted" } else { "deterministic" },
        "journal": journal.as_ref().ok().map(|journal| &journal.path),
        "failedOperation": (!complete).then(|| journal.as_ref().map(|journal| journal.operation).unwrap_or("journal-create")),
        "failedCheck": if complete { None } else { journal.as_ref().ok().and_then(|journal| journal.check) },
        "observations": observations, "complete": complete, "error": outcome.err(),
        "pass": complete && observations.iter().all(|check| check["pass"] == true),
        "report": output.display().to_string()});
    report_file.write_all(&serde_json::to_vec_pretty(&report)?)?;
    report_file.sync_all()?;
    Ok(report)
}
