use crate::specs::{self, tui::common};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
fn invoke(cli: &Path, repo: &Path, home: &Path, args: &[String]) -> Result<Value, String> {
    let out = common::run(
        "node",
        &std::iter::once(cli.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>(),
        Some(repo),
        &common::env_map([("HOME", home.to_string_lossy())]),
        &[],
        None,
        Duration::from_secs(120),
    )?;
    if !out.status.success() {
        return Err(if out.stderr.is_empty() {
            format!("deep-analytics-experiment exited {:?}", out.code())
        } else {
            out.stderr
        });
    }
    let text = out.stdout.trim();
    if !text.starts_with('{') {
        return Err("deep-analytics-experiment must emit one JSON result".into());
    }
    common::parse_json(text, "deep-analytics-experiment")
}
fn identity(v: &Value) -> Result<(), String> {
    if v["ok"] != true
        || v["onboarding"]["product_id"] != "deep-analytics"
        || v["onboarding"]["journey_id"] != "first-use"
        || v["onboarding"]["journey_version"] != "2026-08-04.1"
        || !v["onboarding"]["attempt_id"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    {
        return Err(format!("invalid deep-analytics identity: {v}"));
    }
    Ok(())
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let app = common::required_file(
        context,
        "DEEP_ANALYTICS_APP_ENV",
        "DEEP_ANALYTICS_APP_ENV is required; see the deep-analytics manifest prerequisites",
    )?;
    let echo = common::required_file(
        context,
        "DEEP_ANALYTICS_ECHO_ENV",
        "DEEP_ANALYTICS_ECHO_ENV is required; see the deep-analytics manifest prerequisites",
    )?;
    let repo = Path::new("/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/deep-analytics");
    let cli = repo.join("scripts/analyze-experiment.mjs");
    let home = common::scratch("probierz-deep-analytics")?;
    let subject = format!("probierz-{}", std::process::id());
    let args = |extra: &[&str]| {
        std::iter::once("--onboarding".to_string())
            .chain(extra.iter().map(|s| s.to_string()))
            .chain(["--onboarding-subject".into(), subject.clone()])
            .collect::<Vec<_>>()
    };
    let result = (|| {
        let initial = invoke(&cli, repo, &home, &args(&["--onboarding-reset"]))?;
        identity(&initial)?;
        if initial["onboarding"]["status"] != "in_progress"
            || initial["onboarding"]["screen"]["screen_id"] != "experiment-input"
        {
            return Err(format!("unexpected initial onboarding: {initial}"));
        }
        let attempt = initial["onboarding"]["attempt_id"]
            .as_str()
            .unwrap()
            .to_string();
        for (extra, screen) in [
            (&["--onboarding-next"][..], "analysis-guardrails"),
            (&[][..], "analysis-guardrails"),
            (&["--onboarding-next"][..], "result-interpretation"),
        ] {
            let v = invoke(&cli, repo, &home, &args(extra))?;
            identity(&v)?;
            if v["onboarding"]["attempt_id"] != attempt
                || v["onboarding"]["status"] != "in_progress"
                || v["onboarding"]["screen"]["screen_id"] != screen
            {
                return Err(format!("navigation must not complete first use: {v}"));
            }
        }
        let mut completed_args = vec![
            "--onboarding".into(),
            "--onboarding-subject".into(),
            subject.clone(),
            "--app-env".into(),
            app.to_string_lossy().into_owned(),
            "--echo-env".into(),
            echo.to_string_lossy().into_owned(),
        ];
        if let Some(id) = context.optional("DEEP_ANALYTICS_EXPERIMENT_ID") {
            completed_args.extend(["--experiment-id".into(), id]);
        }
        let completed = invoke(&cli, repo, &home, &completed_args)?;
        identity(&completed)?;
        if completed["onboarding"]["attempt_id"] != attempt
            || completed["onboarding"]["status"] != "completed"
            || completed["onboarding"]["screen"]["screen_id"] != "analytics-result"
            || !completed["rawEvents"].as_i64().is_some_and(|n| n > 0)
            || !completed["summary"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
            || !completed["events"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
        {
            return Err("a real analytics result must contain source events, a non-empty variant summary, and a non-empty event breakdown".into());
        }
        common::write_json(
            &context
                .artifacts
                .join("deep-analytics-onboarding-evidence.json"),
            &json!({"schemaVersion":1,"productId":"deep-analytics","journeyId":"first-use","journeyVersion":"2026-08-04.1","attemptId":attempt,"firstSuccessFact":"analytics_result_observed","evidenceRevision":"analytics-engine-result","experimentId":completed["experimentId"],"rawEvents":completed["rawEvents"],"excludedUserCount":completed["excludedUserCount"],"summary":completed["summary"],"events":completed["events"]}),
        )
    })();
    common::remove(&home);
    result
}
