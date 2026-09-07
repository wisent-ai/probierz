use crate::specs::{self, tui::common};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, time::Duration};
fn invoke(
    cli: &Path,
    args: &[String],
    env: &BTreeMap<String, String>,
    json_result: bool,
) -> Result<Value, String> {
    let mut argv = vec![cli.to_string_lossy().into_owned()];
    argv.extend_from_slice(args);
    let out = common::run("node", &argv, None, env, &[], None, Duration::from_secs(30))?;
    if !out.status.success() {
        return Err(if out.stderr.is_empty() {
            format!("ssh-auth-router exited {:?}", out.code())
        } else {
            out.stderr
        });
    }
    if !json_result {
        return Ok(json!({"stdout":out.stdout.trim()}));
    }
    let text = out.stdout.trim();
    if !text.starts_with('{') {
        return Err("ssh-auth-router must emit one JSON object".into());
    }
    common::parse_json(text, "ssh-auth-router")
}
fn identity(v: &Value) -> Result<(), String> {
    for (p, w) in [
        ("/product_id", "ssh-auth-router"),
        ("/journey_id", "first-use"),
        ("/journey_version", "2026-08-04.1"),
        (
            "/journey_version_id",
            "12000000-0000-4000-8000-000000000006",
        ),
        ("/first_success_fact", "authorized_ssh_decision_observed"),
    ] {
        if v.pointer(p).and_then(Value::as_str) != Some(w) {
            return Err(format!("expected {p}={w}: {v}"));
        }
    }
    Ok(())
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let cli = common::required_file(
        context,
        "SSH_AUTH_ROUTER_CLI",
        "SSH_AUTH_ROUTER_CLI is required by the release scenario",
    )?;
    let onboarding = cli
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new("/"))
        .join("lib/onboarding.mjs");
    let expected_cli = common::required(
        context,
        "SSH_AUTH_ROUTER_EXPECTED_CLI_SHA256",
        "SSH_AUTH_ROUTER_EXPECTED_CLI_SHA256 is required by the release scenario",
    )?
    .to_lowercase();
    let expected_onboarding = common::required(
        context,
        "SSH_AUTH_ROUTER_EXPECTED_ONBOARDING_SHA256",
        "SSH_AUTH_ROUTER_EXPECTED_ONBOARDING_SHA256 is required by the release scenario",
    )?
    .to_lowercase();
    for (name, value) in [
        ("SSH_AUTH_ROUTER_EXPECTED_CLI_SHA256", &expected_cli),
        (
            "SSH_AUTH_ROUTER_EXPECTED_ONBOARDING_SHA256",
            &expected_onboarding,
        ),
    ] {
        if !regex::Regex::new(r"^[0-9a-f]{64}$")
            .unwrap()
            .is_match(value)
        {
            return Err(format!("{name} must be a lowercase SHA-256 digest"));
        }
    }
    let route = common::required(
        context,
        "SSH_AUTH_ROUTER_ROUTE_ID",
        "SSH_AUTH_ROUTER_ROUTE_ID is required by the release scenario",
    )?;
    if common::sha256_file(&cli)? != expected_cli {
        return Err("SSH_AUTH_ROUTER_CLI is not the source-bound release module".into());
    }
    if common::sha256_file(&onboarding)? != expected_onboarding {
        return Err("the CLI loaded an unexpected onboarding module".into());
    }
    let home = common::scratch("probierz-ssh-auth-router")?;
    let env = common::env_map([("XDG_STATE_HOME", home.to_string_lossy())]);
    let state_path = home.join("ssh-auth-router/onboarding.json");
    let result = (|| {
        let fresh = invoke(&cli, &common::strings(&["onboarding", "reset"]), &env, true)?;
        identity(&fresh)?;
        if fresh["status"] != "in_progress"
            || fresh["current_screen_id"] != "authorization-boundary"
        {
            return Err(format!("unexpected fresh state: {fresh}"));
        }
        let state = common::read_json(&state_path)?;
        if state["bundle"]["journey_version_id"] != "12000000-0000-4000-8000-000000000006"
            || state["bundle"]["source_revision"] != "ssh-auth-router-first-use-2026-08-04"
            || state["bundle"]["content_sha256"]
                != "10bb0bf0614c9d2dd67a99fea4471a75028c20c0d482934c8696fab906f25daf"
        {
            return Err("scenario must use the current published canonical bundle".into());
        }
        let attempt = state["progress"]["attempt_id"]
            .as_str()
            .ok_or("attempt id must be retained")?
            .to_string();
        let advanced = invoke(
            &cli,
            &common::strings(&["onboarding", "advance"]),
            &env,
            true,
        )?;
        identity(&advanced)?;
        if advanced["status"] != "in_progress"
            || advanced["current_screen_id"] != "authorized-decision"
        {
            return Err("navigation must not complete first use".into());
        }
        let resumed = invoke(&cli, &common::strings(&["onboarding", "show"]), &env, true)?;
        if resumed["status"] != "in_progress" {
            return Err("resume must retain the attempt".into());
        }
        let unknown = invoke(
            &cli,
            &common::strings(&["probe", "--route=probierz-navigation-negative"]),
            &env,
            false,
        )?;
        if unknown["stdout"] != "" {
            return Err("an unknown route must not fabricate a decision".into());
        }
        if invoke(
            &cli,
            &common::strings(&["onboarding", "status"]),
            &env,
            true,
        )?["status"]
            != "in_progress"
        {
            return Err("unresolved route selection must not complete first use".into());
        }
        let probe = invoke(
            &cli,
            &vec!["probe".into(), format!("--route={route}")],
            &env,
            true,
        )?;
        if probe["id"] != route
            || probe["ok"] != true
            || probe["skipped"] == true
            || probe["status"] != 0
            || probe["stdout"] != "ok"
        {
            return Err("the configured route must return a real authorized decision".into());
        }
        let completed = invoke(
            &cli,
            &common::strings(&["onboarding", "status"]),
            &env,
            true,
        )?;
        identity(&completed)?;
        if completed["status"] != "completed"
            || completed["current_screen_id"] != "authorized-decision"
        {
            return Err("authorized decision did not complete first use".into());
        }
        let state = common::read_json(&state_path)?;
        let events = state["pending_events"]
            .as_array()
            .ok_or("pending events missing")?;
        if !events.iter().any(|e| {
            e["attempt_id"] == attempt && e["event_name"] == "onboarding_first_success_observed"
        }) {
            return Err("durable canonical first-success evidence is required".into());
        }
        let rev = state["progress"]["evidence_revision"]
            .as_str()
            .ok_or("evidence revision missing")?;
        common::write_trace(
            context,
            "ssh-auth-router-onboarding-first-use.trace.json",
            json!({"schemaVersion":1,"kind":"probierz-first-use-trace","productId":"ssh-auth-router","journeyId":"first-use","journeyVersion":"2026-08-04.1","journeyVersionId":"12000000-0000-4000-8000-000000000006","sourceRevision":"ssh-auth-router-first-use-2026-08-04","firstSuccessFact":"authorized_ssh_decision_observed","attemptId":attempt,"evidenceRevision":rev,"evidenceRevisionSha256":hex::encode(Sha256::digest(rev.as_bytes())),"navigationNegative":{"unknownRoute":true,"statusBeforeSuccess":"in_progress"},"resultReceipt":{"routeIdSha256":hex::encode(Sha256::digest(route.as_bytes())),"decision":"authorized","status":0,"stdoutSha256":hex::encode(Sha256::digest(b"ok"))},"terminalScreenId":"authorized-decision","status":"completed","requestAcceptanceCompletesJourney":false}),
        )
    })();
    common::remove(&home);
    result
}
