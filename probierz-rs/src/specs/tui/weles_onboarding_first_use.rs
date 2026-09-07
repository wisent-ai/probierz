use crate::specs::{self, tui::common};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, time::Duration};
fn invoke(cli: &Path, args: &[String]) -> Result<String, String> {
    let command = if cli.extension().and_then(|value| value.to_str()) == Some("js") {
        "node".to_string()
    } else {
        cli.to_string_lossy().into_owned()
    };
    let argv = if command == "node" {
        std::iter::once(cli.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .collect()
    } else {
        args.to_vec()
    };
    let out = common::run(
        &command,
        &argv,
        None,
        &BTreeMap::new(),
        &[],
        None,
        Duration::from_secs(45),
    )?;
    if !out.status.success() {
        return Err(if out.stderr.is_empty() {
            format!("weles exited {:?}", out.code())
        } else {
            out.stderr
        });
    }
    Ok(out.stdout.trim().into())
}
fn identity(v: &Value) -> Result<(), String> {
    if v["product_id"] != "weles"
        || v["journey_id"] != "first-use"
        || v["journey_version"] != "2026-08-04.1"
        || !v["attempt_id"].as_str().is_some_and(|s| !s.is_empty())
    {
        return Err(format!("invalid weles onboarding identity: {v}"));
    }
    Ok(())
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let cli = common::required_file(
        context,
        "WELES_CLI",
        "WELES_CLI is required; see the Weles manifest prerequisites",
    )?;
    let digest = common::required(
        context,
        "WELES_EXPECTED_CLI_SHA256",
        "WELES_EXPECTED_CLI_SHA256 must be the published release executable digest",
    )?;
    if !regex::Regex::new(r"(?i)^[0-9a-f]{64}$")
        .unwrap()
        .is_match(&digest)
    {
        return Err(
            "WELES_EXPECTED_CLI_SHA256 must be the published release executable digest".into(),
        );
    }
    if common::sha256_file(&cli)? != digest.to_lowercase() {
        return Err("WELES_CLI does not match the source-bound release digest".into());
    }
    let receipt = common::required_file(
        context,
        "WELES_RECEIPT_PATH",
        "WELES_RECEIPT_PATH is required; see the Weles manifest prerequisites",
    )?;
    let keys = common::required_file(
        context,
        "WELES_RECEIPT_KEYS_PATH",
        "WELES_RECEIPT_KEYS_PATH is required; see the Weles manifest prerequisites",
    )?;
    let state = common::scratch("probierz-weles-onboarding")?;
    let subject = format!("probierz-{}", std::process::id());
    let onboard = |action: &str, extra: &[String]| {
        let mut a = vec![
            "onboarding".into(),
            action.into(),
            "--subject".into(),
            subject.clone(),
            "--state-dir".into(),
            state.to_string_lossy().into_owned(),
        ];
        a.extend_from_slice(extra);
        let text = invoke(&cli, &a)?;
        if !text.starts_with('{') {
            return Err("weles onboarding must emit one JSON view".into());
        }
        common::parse_json(&text, "weles onboarding")
    };
    let result = (|| {
        if invoke(&cli, &["version".into()])? != "0.4.0" {
            return Err("WELES_CLI must be the expected release version".into());
        }
        let initial = onboard("reset", &[])?;
        identity(&initial)?;
        if initial["status"] != "in_progress" || initial["screen"]["id"] != "authorization-boundary"
        {
            return Err(format!("unexpected fresh state: {initial}"));
        }
        let attempt = initial["attempt_id"].as_str().unwrap().to_string();
        for (action, screen) in [
            ("next", "host-execution"),
            ("status", "host-execution"),
            ("next", "receipt-verification"),
        ] {
            let v = onboard(action, &[])?;
            identity(&v)?;
            if v["attempt_id"] != attempt
                || v["status"] != "in_progress"
                || v["screen"]["id"] != screen
            {
                return Err(
                    "navigation and workflow request acceptance must not complete first use".into(),
                );
            }
        }
        let completed = onboard(
            "verify",
            &[
                "--receipt".into(),
                receipt.to_string_lossy().into_owned(),
                "--keys".into(),
                keys.to_string_lossy().into_owned(),
            ],
        )?;
        identity(&completed)?;
        let outcome = completed["verified_receipt"]["outcome"]
            .as_str()
            .unwrap_or("");
        if completed["status"] != "completed"
            || completed["screen"]["id"] != "receipt-verification"
            || outcome.is_empty()
            || regex::Regex::new(r"(?i)^(accepted|queued|pending|running|submitted)$")
                .unwrap()
                .is_match(outcome)
            || !completed["verified_receipt"]["evidence_digest"]
                .as_str()
                .is_some_and(|s| {
                    regex::Regex::new(r"(?i)^[0-9a-f]{64}$")
                        .unwrap()
                        .is_match(s)
                })
        {
            return Err("a request-acceptance outcome is not a terminal workflow result".into());
        }
        let persisted = onboard("status", &[])?;
        if persisted["status"] != "completed" {
            return Err("verified receipt completion must persist across restart/resume".into());
        }
        common::write_json(
            &context.artifacts.join("weles-onboarding-evidence.json"),
            &json!({"schemaVersion":1,"productId":"weles","journeyId":"first-use","journeyVersion":"2026-08-04.1","cliSha256":digest,"receiptSha256":common::sha256_file(&receipt)?,"trustedKeysSha256":common::sha256_file(&keys)?,"attemptId":attempt,"firstSuccessFact":"workflow_receipt_verified","terminalScreenId":"receipt-verification","status":"completed","verifiedReceipt":completed["verified_receipt"]}),
        )
    })();
    common::remove(&state);
    result
}
