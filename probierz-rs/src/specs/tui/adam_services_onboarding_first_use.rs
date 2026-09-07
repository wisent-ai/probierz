use crate::specs::{self, tui::common};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
fn adapter(
    python: &str,
    repo: &Path,
    env: &BTreeMap<String, String>,
    attempt: &str,
    action: &str,
    input: Option<&Value>,
) -> Result<Value, String> {
    let script = r#"import json,sys
from onboarding import AdamOnboardingRuntime
r=AdamOnboardingRuntime(); a,i=sys.argv[1],sys.argv[2]; p=json.load(sys.stdin) if a in {'discover','observe'} else None
x=r.start(i) if a=='start' else r.discover(i,p['services']) if a=='discover' else r.observe_result(i,p['idempotency_key'],p['response']) if a=='observe' else r.state(i)
print(json.dumps(x,sort_keys=True,separators=(',',':')))"#;
    let args = vec!["-c".into(), script.into(), action.into(), attempt.into()];
    let body = input.map(|v| v.to_string());
    let out = common::run(
        python,
        &args,
        Some(repo),
        env,
        &["STADO_ONBOARDING_TOKEN"],
        body.as_deref(),
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
    common::parse_json(&out.stdout, "process")
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let repo = Path::new("/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/adam-services");
    let python = common::required(
        context,
        "ADAM_SERVICES_PYTHON",
        "ADAM_SERVICES_PYTHON is required: provide an absolute approved Python executable",
    )?;
    if !Path::new(&python).is_absolute() {
        return Err("ADAM_SERVICES_PYTHON must be absolute".into());
    }
    let build = common::required(
        context,
        "PROBIERZ_BUILD_PATH",
        "PROBIERZ_BUILD_PATH is required: provide the exact release-bound product entry point",
    )?;
    if PathBuf::from(&build).canonicalize().ok() != repo.join("onboarding.py").canonicalize().ok() {
        return Err(
            "PROBIERZ_BUILD_PATH must identify the product entry point exercised by this scenario"
                .into(),
        );
    }
    let base = common::required(
        context,
        "ADAM_SERVICES_BASE_URL",
        "ADAM_SERVICES_BASE_URL is required: provide the externally provisioned release service",
    )?;
    if !base.starts_with("https://") {
        return Err("ADAM_SERVICES_BASE_URL must use HTTPS".into());
    }
    let token = common::required(
        context,
        "ADAM_SERVICES_AUTH_TOKEN",
        "ADAM_SERVICES_AUTH_TOKEN is required: provide a scenario-scoped service credential",
    )?;
    let temp = common::scratch("probierz-adam-services")?;
    let state_path = temp.join("onboarding-state.json");
    let attempt = format!("00000000-0000-4000-8000-{:012}", std::process::id());
    let idem = format!("probierz-{attempt}");
    let env = common::env_map([
        ("PYTHONDONTWRITEBYTECODE", "1"),
        (
            "ADAM_ONBOARDING_STATE_PATH",
            state_path.to_string_lossy().as_ref(),
        ),
    ]);
    let result = (|| {
        let initial = adapter(&python, repo, &env, &attempt, "start", None)?;
        if initial["journey"]["product_id"] != "adam-services"
            || initial["journey"]["journey_id"] != "first-use"
            || initial["journey"]["journey_version"] != "2026-08-04.1"
            || initial["attempt"]["current_screen_id"] != "discover_api"
            || initial["attempt"]["completed"] != false
        {
            return Err(format!("unexpected initial Adam Services state: {initial}"));
        }
        let resumed = adapter(&python, repo, &env, &attempt, "start", None)?;
        if resumed["attempt"]["attempt_id"] != attempt || resumed["attempt"]["completed"] != false {
            return Err("a new adapter process must resume the same attempt".into());
        }
        let capabilities: Value =
            ureq::get(&format!("{}/capabilities", base.trim_end_matches('/')))
                .set("Authorization", &format!("Bearer {token}"))
                .timeout(Duration::from_secs(120))
                .call()
                .map_err(|e| {
                    format!("the authorized normal capabilities route must return 200: {e}")
                })?
                .into_json()
                .map_err(|e| e.to_string())?;
        let services = capabilities["services"]
            .as_array()
            .ok_or("capabilities services must be an array")?;
        if !services.iter().any(|s| s["name"] == "summarize") {
            return Err("capabilities must contain summarize".into());
        }
        let discovered = adapter(
            &python,
            repo,
            &env,
            &attempt,
            "discover",
            Some(&json!({"services":services})),
        )?;
        if discovered["attempt"]["current_screen_id"] != "run_authenticated_request"
            || discovered["attempt"]["completed"] != false
        {
            return Err("discovery and navigation must not complete first use".into());
        }
        let request = json!({"text":"Probierz confirms that Adam Services returns a real structured summary through the normal API.","max_points":3,"style":"bullet"});
        let url = format!("{}/summarize", base.trim_end_matches('/'));
        let rejected = ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_json(request.clone());
        let rejected_status = match rejected {
            Err(ureq::Error::Status(code, _)) => code,
            Ok(r) => r.status(),
            Err(e) => return Err(e.to_string()),
        };
        if ![401, 403].contains(&rejected_status) {
            return Err("the same API operation must reject an unauthenticated request".into());
        }
        let response: Value = ureq::post(&url)
            .set("Authorization", &format!("Bearer {token}"))
            .set("Content-Type", "application/json")
            .timeout(Duration::from_secs(120))
            .send_json(request)
            .map_err(|e| format!("the authorized normal summarize operation must return 200: {e}"))?
            .into_json()
            .map_err(|e| e.to_string())?;
        if response["success"] != true
            || response["service"] != "summarize"
            || !response["result"]
                .as_object()
                .is_some_and(|o| !o.is_empty())
        {
            return Err("the service must return a non-empty structured result".into());
        }
        let completed = adapter(
            &python,
            repo,
            &env,
            &attempt,
            "observe",
            Some(&json!({"idempotency_key":idem,"response":response})),
        )?;
        if completed["onboarding"]["completed"] != true
            || completed["onboarding"]["current_screen_id"] != "result_observed"
        {
            return Err("authenticated API result did not complete first use".into());
        }
        let state = common::read_json(&state_path)?;
        let events = state["pending_events"]
            .as_array()
            .ok_or("pending events missing")?;
        for name in [
            "onboarding_resumed",
            "onboarding_first_success_observed",
            "onboarding_completed",
        ] {
            if !events
                .iter()
                .any(|e| e["attempt_id"] == attempt && e["event_name"] == name)
            {
                return Err(format!("missing {name}"));
            }
        }
        common::write_json(
            &context
                .artifacts
                .join("adam-services-onboarding-first-use.trace.json"),
            &json!({"schemaVersion":1,"kind":"probierz-onboarding-e2-trace","productId":"adam-services","journeyVersionId":"12000000-0000-4000-8000-000000000003","sourceRevision":"adam-services-first-use-2026-08-04","attemptId":attempt,"firstSuccessFact":"authenticated_api_result_observed","terminalScreenId":"result_observed","status":"completed","observation":{"unauthenticatedStatus":rejected_status,"authenticatedStatus":200,"service":"summarize","persistedCompletion":true}}),
        )
    })();
    common::remove(&temp);
    result
}
