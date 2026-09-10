use crate::specs::{self, tui::common};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, time::Duration};
fn py(
    python: &str,
    args: &[String],
    root: &Path,
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> Result<common::Output, String> {
    let out = common::run(
        python,
        args,
        Some(root),
        env,
        &[
            "STADO_INTEGRATION_API_URL",
            "WISENT_OPTIMIZER_STADO_INTEGRATION_TOKEN",
        ],
        None,
        timeout,
    )?;
    if !out.status.success() {
        return Err(format!(
            "optimizer process exited {:?}\nstdout:\n{}\nstderr:\n{}",
            out.code(),
            out.stdout,
            out.stderr
        ));
    }
    Ok(out)
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let req = |n: &str| {
        common::required(context,n,&format!("{n} is required; Probierz will not invent optimizer model, task, search-space, or interpreter coordinates"))
    };
    let names = [
        "PROBIERZ_WISENT_OPTIMIZER_PYTHON",
        "PROBIERZ_WISENT_OPTIMIZER_MODEL",
        "PROBIERZ_WISENT_OPTIMIZER_TASK",
        "PROBIERZ_WISENT_OPTIMIZER_METHODS_JSON",
        "PROBIERZ_WISENT_OPTIMIZER_STRENGTHS_JSON",
        "PROBIERZ_WISENT_OPTIMIZER_LAYER_RANGE",
        "PROBIERZ_WISENT_OPTIMIZER_LIMIT",
        "PROBIERZ_WISENT_OPTIMIZER_MAX_TIME_MINUTES",
        "PROBIERZ_WISENT_OPTIMIZER_MIN_NORM_THRESHOLD",
        "PROBIERZ_WISENT_OPTIMIZER_ARCHITECTURE_MODULE_LIMIT",
        "PROBIERZ_WISENT_OPTIMIZER_PROGRESS_LOG_INTERVAL",
        "PROBIERZ_WISENT_OPTIMIZER_TRAIN_RATIO",
    ];
    let mut values = BTreeMap::new();
    for n in names {
        values.insert(n, req(n)?);
    }
    let methods: serde_json::Value =
        serde_json::from_str(&values["PROBIERZ_WISENT_OPTIMIZER_METHODS_JSON"]).map_err(|e| {
            format!("PROBIERZ_WISENT_OPTIMIZER_METHODS_JSON must be a JSON array: {e}")
        })?;
    if !methods
        .as_array()
        .is_some_and(|a| !a.is_empty() && a.iter().all(|x| x.is_string()))
    {
        return Err("PROBIERZ_WISENT_OPTIMIZER_METHODS_JSON must be a non-empty array".into());
    }
    let strengths: serde_json::Value =
        serde_json::from_str(&values["PROBIERZ_WISENT_OPTIMIZER_STRENGTHS_JSON"]).map_err(|e| {
            format!("PROBIERZ_WISENT_OPTIMIZER_STRENGTHS_JSON must be a JSON array: {e}")
        })?;
    if !strengths
        .as_array()
        .is_some_and(|a| !a.is_empty() && a.iter().all(|x| x.is_number()))
    {
        return Err("PROBIERZ_WISENT_OPTIMIZER_STRENGTHS_JSON must be a non-empty array".into());
    }
    if !regex::Regex::new(r"^\d+(?:-\d+|(?:,\d+)*)$")
        .unwrap()
        .is_match(&values["PROBIERZ_WISENT_OPTIMIZER_LAYER_RANGE"])
    {
        return Err("PROBIERZ_WISENT_OPTIMIZER_LAYER_RANGE has invalid syntax".into());
    }
    let ratio: f64 = values["PROBIERZ_WISENT_OPTIMIZER_TRAIN_RATIO"]
        .parse()
        .map_err(|_| "PROBIERZ_WISENT_OPTIMIZER_TRAIN_RATIO must be between zero and one")?;
    if !(0.0 < ratio && ratio < 1.0) {
        return Err("PROBIERZ_WISENT_OPTIMIZER_TRAIN_RATIO must be between zero and one".into());
    }
    let root =
        Path::new("/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/wisent-optimizer");
    let temp = common::scratch("probierz-wisent-optimizer-first-use")?;
    let state_home = temp.join("state");
    let result_path = temp.join("ranked-result.json");
    let mut env = values
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect::<BTreeMap<_, _>>();
    env.extend(common::env_map([
        ("XDG_STATE_HOME", state_home.to_string_lossy().as_ref()),
        (
            "PYTHONPATH",
            format!(
                "{}:{}",
                root.display(),
                std::env::var("PYTHONPATH").unwrap_or_default()
            )
            .as_str(),
        ),
        (
            "WISENT_OPTIMIZER_ONBOARDING_SUBJECT",
            "probierz-isolated-wisent-optimizer-first-use",
        ),
        (
            "PROBIERZ_OPTIMIZER_RESULT_PATH",
            result_path.to_string_lossy().as_ref(),
        ),
    ]));
    let state_path = state_home.join("wisent-optimizer/onboarding.json");
    let onboard = |action: &str| {
        py(
            &values["PROBIERZ_WISENT_OPTIMIZER_PYTHON"],
            &vec![
                "-m".into(),
                "wisent.core.control.steering_optimizer.onboarding".into(),
                "onboarding".into(),
                "--subject".into(),
                "probierz-isolated-wisent-optimizer-first-use".into(),
                action.into(),
            ],
            root,
            &env,
            Duration::from_secs(120),
        )
    };
    let result = (|| {
        onboard("--status")?;
        let mut state = common::read_json(&state_path)?;
        let progress = state["progress"]
            .as_object()
            .and_then(|m| m.values().next())
            .ok_or("durable progress must be keyed by the hashed isolated subject")?;
        let attempt = progress["attempt_id"]
            .as_str()
            .ok_or("attempt id missing")?
            .to_string();
        if progress["current_screen_id"] != "search-space" || progress["status"] != "in_progress" {
            return Err("fresh optimizer state is not in progress at search-space".into());
        }
        onboard("--advance")?;
        onboard("--advance")?;
        onboard("--advance")?;
        let blocked = onboard("--advance")?;
        if !blocked
            .stdout
            .to_lowercase()
            .contains("cannot be completed by a click")
        {
            return Err("expected cannot be completed by a click".into());
        }
        let program = r#"import json,os
from pathlib import Path
from wisent.core.control.steering_optimizer.optimizer_cli import run_steering_optimization
r=run_steering_optimization(model_name=os.environ['PROBIERZ_WISENT_OPTIMIZER_MODEL'],task_name=os.environ['PROBIERZ_WISENT_OPTIMIZER_TASK'],limit=int(os.environ['PROBIERZ_WISENT_OPTIMIZER_LIMIT']),min_norm_threshold=float(os.environ['PROBIERZ_WISENT_OPTIMIZER_MIN_NORM_THRESHOLD']),optimization_type='method_comparison',methods_to_test=json.loads(os.environ['PROBIERZ_WISENT_OPTIMIZER_METHODS_JSON']),layer_range=os.environ['PROBIERZ_WISENT_OPTIMIZER_LAYER_RANGE'],strength_range=json.loads(os.environ['PROBIERZ_WISENT_OPTIMIZER_STRENGTHS_JSON']),device=os.environ.get('PROBIERZ_WISENT_OPTIMIZER_DEVICE') or None,verbose=False,max_time_minutes=float(os.environ['PROBIERZ_WISENT_OPTIMIZER_MAX_TIME_MINUTES']),min_clusters=None,tecza_params=None,architecture_module_limit=int(os.environ['PROBIERZ_WISENT_OPTIMIZER_ARCHITECTURE_MODULE_LIMIT']),progress_log_interval=int(os.environ['PROBIERZ_WISENT_OPTIMIZER_PROGRESS_LOG_INTERVAL']),train_ratio=float(os.environ['PROBIERZ_WISENT_OPTIMIZER_TRAIN_RATIO']))
Path(os.environ['PROBIERZ_OPTIMIZER_RESULT_PATH']).write_text(json.dumps(r,sort_keys=True,default=str))"#;
        py(
            &values["PROBIERZ_WISENT_OPTIMIZER_PYTHON"],
            &vec!["-c".into(), program.into()],
            root,
            &env,
            Duration::from_secs(3600),
        )?;
        let ranked = common::read_json(&result_path)?;
        if !ranked["method_ranking"]
            .as_object()
            .is_some_and(|m| !m.is_empty())
            || !ranked["total_configurations_tested"]
                .as_i64()
                .is_some_and(|n| n > 0)
        {
            return Err("optimizer produced no ranked configuration".into());
        }
        state = common::read_json(&state_path)?;
        let progress = state["progress"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap();
        if progress["attempt_id"] != attempt
            || progress["status"] != "completed"
            || progress["evidence"]["ranked_configuration_observed"] != true
        {
            return Err("ranked configuration did not complete onboarding".into());
        }
        let success = state["events"]
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|e| e["event_name"] == "onboarding_first_success_observed")
            })
            .ok_or("canonical first-success event must be retained offline")?;
        common::write_json(
            &context
                .artifacts
                .join("wisent-optimizer-onboarding-first-use.trace.json"),
            &json!({"schemaVersion":1,"productId":"wisent-optimizer","journeyId":"first-use","journeyVersion":"2026-08-04.1","journeyVersionId":state["bundles"]["first-use"]["journey_version_id"],"firstSuccessFact":"ranked_configuration_observed","attemptId":attempt,"evidenceRevision":progress["evidence_revision"],"completionEventId":success["event_id"],"observation":progress["evidence"]["ranked_configuration"]}),
        )
    })();
    common::remove(&temp);
    result
}
