use crate::specs::{self, tui::common};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
fn invoke(
    python: &str,
    repo: &Path,
    env: &BTreeMap<String, String>,
    op: &str,
) -> Result<Value, String> {
    let out = common::run(
        python,
        &vec!["-m".into(), "adam_toolkit.onboarding".into(), op.into()],
        Some(repo),
        env,
        &[
            "STADO_INTEGRATION_API_URL",
            "ADAM_AGENT_TOOLKIT_STADO_INTEGRATION_TOKEN",
        ],
        None,
        Duration::from_secs(120),
    )?;
    if !out.status.success() {
        return Err(format!(
            "process exited {:?}\nstdout:\n{}\nstderr:\n{}",
            out.code(),
            out.stdout,
            out.stderr
        ));
    }
    let payload = common::parse_json(&out.stdout, "process")?;
    if payload["ok"] != true {
        return Err(format!("expected ok true: {payload}"));
    }
    Ok(payload["result"].clone())
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let repo =
        Path::new("/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/adam-agent-toolkit");
    let python = common::required(
        context,
        "ADAM_AGENT_TOOLKIT_PYTHON",
        "ADAM_AGENT_TOOLKIT_PYTHON is required: provide an absolute approved Python executable",
    )?;
    if !Path::new(&python).is_absolute() {
        return Err("ADAM_AGENT_TOOLKIT_PYTHON must be absolute".into());
    }
    let build = common::required(
        context,
        "PROBIERZ_BUILD_PATH",
        "PROBIERZ_BUILD_PATH is required: provide the exact release-bound product entry point",
    )?;
    let expected = repo.join("adam_toolkit/onboarding.py");
    if PathBuf::from(&build).canonicalize().ok() != expected.canonicalize().ok() {
        return Err(
            "PROBIERZ_BUILD_PATH must identify the product entry point exercised by this scenario"
                .into(),
        );
    }
    let temp = common::scratch("probierz-adam-agent-toolkit")?;
    let env = common::env_map([
        ("PYTHONDONTWRITEBYTECODE", "1"),
        (
            "ADAM_AGENT_TOOLKIT_ONBOARDING_STATE",
            temp.join("onboarding.json").to_string_lossy().as_ref(),
        ),
        (
            "ADAM_AGENT_TOOLKIT_ONBOARDING_SUBJECT",
            format!("probierz-{}", std::process::id()).as_str(),
        ),
    ]);
    let result = (|| {
        let initial = invoke(&python, repo, &env, "reset")?;
        for (p, w) in [
            ("/product_id", "adam-agent-toolkit"),
            ("/journey_id", "first-use"),
            ("/journey_version", "2026-08-04.1"),
            (
                "/journey_version_id",
                "12000000-0000-4000-8000-000000000002",
            ),
            (
                "/source_revision",
                "adam-agent-toolkit-first-use-2026-08-04",
            ),
            ("/status", "in_progress"),
            ("/screen/screen_id", "discover-tools"),
        ] {
            if initial.pointer(p).and_then(Value::as_str) != Some(w) {
                return Err(format!("expected {p}={w}: {initial}"));
            }
        }
        let attempt = initial["attempt_id"]
            .as_str()
            .ok_or("initial attempt_id must be a string")?
            .to_string();
        let resumed = invoke(&python, repo, &env, "status")?;
        if resumed["attempt_id"] != attempt
            || resumed["status"] != "in_progress"
            || resumed["screen"]["screen_id"] != "discover-tools"
        {
            return Err("a separate process must resume the same fresh-subject attempt".into());
        }
        let discovered = invoke(&python, repo, &env, "discover")?;
        if discovered["onboarding"]["attempt_id"] != attempt
            || discovered["onboarding"]["status"] != "in_progress"
            || discovered["onboarding"]["screen"]["screen_id"] != "call-tool"
        {
            return Err("catalog discovery and navigation must not complete first use".into());
        }
        if discovered["catalog"]["operation"] != "discover_local_tools"
            || !discovered["catalog"]["tools"]
                .as_array()
                .is_some_and(|a| a.iter().any(|t| t["tool_id"] == "cost_tracker.summary"))
        {
            return Err("catalog must discover cost_tracker.summary".into());
        }
        let completed = invoke(&python, repo, &env, "call")?;
        if completed["onboarding"]["status"] != "completed"
            || completed["tool_call"]["tool_id"] != "cost_tracker.summary"
            || completed["tool_call"]["result"]["runway_hours"] != 24
        {
            return Err(format!("unexpected real local tool result: {completed}"));
        }
        let persisted = invoke(&python, repo, &env, "status")?;
        if persisted["status"] != "completed" {
            return Err("completion must survive a process boundary".into());
        }
        let state = common::read_json(&temp.join("onboarding.json"))?;
        let events = state["events"]
            .as_array()
            .ok_or("durable canonical event evidence is required")?;
        for name in [
            "onboarding_resumed",
            "onboarding_first_success_observed",
            "onboarding_completed",
        ] {
            if !events
                .iter()
                .any(|e| e["attempt_id"] == attempt && e["event_name"] == name)
            {
                return Err(format!("missing canonical event {name}"));
            }
        }
        common::write_json(
            &context
                .artifacts
                .join("adam-agent-toolkit-onboarding-first-use.trace.json"),
            &json!({"schemaVersion":1,"kind":"probierz-onboarding-e2-trace","productId":"adam-agent-toolkit","journeyVersionId":"12000000-0000-4000-8000-000000000002","sourceRevision":"adam-agent-toolkit-first-use-2026-08-04","attemptId":attempt,"firstSuccessFact":"local_tool_result_observed","terminalScreenId":"call-tool","status":"completed","observation":{"toolId":"cost_tracker.summary","balance":12,"effectiveBurnRate":0.5,"runwayHours":24,"persistedCompletion":true}}),
        )
    })();
    common::remove(&temp);
    result
}
