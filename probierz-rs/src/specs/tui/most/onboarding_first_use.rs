use crate::specs::{self, tui::common};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
fn executable(context: &specs::Context, name: &str) -> Result<PathBuf, String> {
    let path = common::required_file(
        context,
        name,
        &format!("{name} is required; see the Most manifest prerequisites"),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path
            .metadata()
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err(format!("{name} must be executable"));
        }
    }
    Ok(path)
}
fn invoke(
    command: &Path,
    args: &[String],
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> Result<String, String> {
    let out = common::run(
        command.to_string_lossy().as_ref(),
        args,
        None,
        env,
        &[],
        None,
        timeout,
    )?;
    if !out.status.success() {
        return Err(format!(
            "{} {} exited unsuccessfully",
            command.display(),
            args.join(" ")
        ));
    }
    Ok(out.stdout.trim().into())
}
fn identity(v: &Value) -> Result<(), String> {
    if v["product_id"] != "most"
        || v["journey_id"] != "first-use"
        || v["journey_version"] != "2026-08-04.1"
        || v["first_success_fact"] != "provider_receipt_received"
        || v["request_acceptance_completes_journey"] != false
        || v["current_screen_id"] != "provider-receipt"
    {
        return Err(format!("invalid Most onboarding identity: {v}"));
    }
    Ok(())
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let cli = executable(context, "MOST_CLI")?;
    let workflow = executable(context, "MOST_PROVIDER_WORKFLOW")?;
    let base = common::required(
        context,
        "MOST_BASE_URL",
        "MOST_BASE_URL is required; see the Most manifest prerequisites",
    )?;
    let token = common::required(
        context,
        "MOST_AGENT_API_TOKEN",
        "MOST_AGENT_API_TOKEN is required; see the Most manifest prerequisites",
    )?;
    let revision = common::required(
        context,
        "MOST_EXPECTED_SOURCE_REVISION",
        "MOST_EXPECTED_SOURCE_REVISION is required; see the Most manifest prerequisites",
    )?;
    if revision.eq_ignore_ascii_case("unknown") {
        return Err("MOST_EXPECTED_SOURCE_REVISION must identify the release source".into());
    }
    let timeout = context
        .optional("MOST_PROVIDER_RECEIPT_TIMEOUT_MS")
        .unwrap_or_else(|| "120000".into())
        .parse::<u64>()
        .map_err(|_| {
            "MOST_PROVIDER_RECEIPT_TIMEOUT_MS must be an integer from 10000 through 240000"
        })?;
    if !(10000..=240000).contains(&timeout) {
        return Err(
            "MOST_PROVIDER_RECEIPT_TIMEOUT_MS must be an integer from 10000 through 240000".into(),
        );
    }
    let env = common::env_map([
        ("MOST_BASE_URL", base.as_str()),
        ("MOST_AGENT_API_TOKEN", token.as_str()),
    ]);
    let onboarding = || {
        let text = invoke(&cli, &["onboarding".into()], &env, Duration::from_secs(45))?;
        if !text.starts_with('{') {
            return Err("most-cli onboarding must emit the API JSON response".into());
        }
        common::parse_json(&text, "most-cli onboarding")
    };
    if invoke(&cli, &["--version".into()], &env, Duration::from_secs(15))?
        != format!("most-cli 0.1.0 ({revision})")
    {
        return Err("MOST_CLI must be the expected source-bound release".into());
    }
    let initial = onboarding()?;
    identity(&initial)?;
    if initial["status"] != "in_progress" {
        return Err("the prerequisite requires a fresh dedicated Most service subject".into());
    }
    if !initial["next"]
        .as_str()
        .is_some_and(|s| s.contains("message.delivered or message.read"))
    {
        return Err("next must name message.delivered or message.read".into());
    }
    let resumed = onboarding()?;
    if resumed["status"] != "in_progress" {
        return Err("re-reading the accepted onboarding request must resume, not complete".into());
    }
    let submitted = common::run(
        workflow.to_string_lossy().as_ref(),
        &[],
        None,
        &common::env_map([("MOST_BASE_URL", base.as_str())]),
        &[],
        None,
        Duration::from_millis(timeout.min(120000)),
    )?;
    if !submitted.status.success() {
        return Err("the approved provider workflow did not submit its real message".into());
    }
    let deadline = Instant::now() + Duration::from_millis(timeout);
    let completed = loop {
        let v = onboarding()?;
        identity(&v)?;
        if v["status"] == "completed" {
            break v;
        }
        if v["status"] != "in_progress" {
            return Err("Most entered an unexpected pre-receipt state".into());
        }
        if Instant::now() >= deadline {
            return Err("Most did not observe a provider-originated message.delivered or message.read receipt before the authorized timeout".into());
        }
        thread::sleep(Duration::from_secs(2));
    };
    if completed["next"] != "Complete: Most observed a real provider delivery/read receipt." {
        return Err(format!("unexpected completion: {completed}"));
    }
    if onboarding()?["status"] != "completed" {
        return Err(
            "provider receipt completion must persist across the API/CLI resume boundary".into(),
        );
    }
    common::write_json(
        &context.artifacts.join("most-onboarding-evidence.json"),
        &json!({"schemaVersion":1,"productId":"most","journeyId":"first-use","journeyVersion":"2026-08-04.1","sourceRevision":revision,"cliSha256":common::sha256_file(&cli)?,"providerWorkflowSha256":common::sha256_file(&workflow)?,"firstSuccessFact":"provider_receipt_received","terminalScreenId":"provider-receipt","status":"completed","requestAcceptanceCompletesJourney":false,"canonicalObservation":completed["next"]}),
    )
}
