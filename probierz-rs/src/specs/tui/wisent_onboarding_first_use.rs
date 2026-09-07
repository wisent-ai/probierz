use crate::specs::{self, tui::common};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, time::Duration};
fn invoke(
    python: &str,
    args: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> Result<common::Output, String> {
    let mut argv = vec![
        "-m".into(),
        "wisent.core.primitives.model_interface.core.main".into(),
    ];
    argv.extend_from_slice(args);
    let out = common::run(
        python,
        &argv,
        Some(cwd),
        env,
        &[
            "STADO_INTEGRATION_API_URL",
            "WISENT_STADO_INTEGRATION_TOKEN",
        ],
        None,
        timeout,
    )?;
    if !out.status.success() {
        return Err(format!(
            "Wisent exited {:?}\nstdout:\n{}\nstderr:\n{}",
            out.code(),
            out.stdout,
            out.stderr
        ));
    }
    Ok(out)
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let required = |n: &str| {
        common::required(context,n,&format!("{n} is required; Probierz will not invent model, task, dataset, output, interpreter, or workspace coordinates"))
    };
    let python = required("PROBIERZ_WISENT_PYTHON")?;
    let workdir = required("PROBIERZ_WISENT_WORKDIR")?;
    let args_text = required("PROBIERZ_WISENT_OPERATION_ARGS_JSON")?;
    let operation: Vec<String> = serde_json::from_str(&args_text).map_err(|e| {
        format!("PROBIERZ_WISENT_OPERATION_ARGS_JSON must be a JSON argv array: {e}")
    })?;
    if operation.is_empty() {
        return Err("a real representation operation argv is required".into());
    }
    let allowed = [
        "tasks",
        "generate-pairs",
        "generate-pairs-from-task",
        "get-activations",
        "create-steering-vector",
        "generate-vector-from-task",
        "generate-vector-from-synthetic",
        "synthetic",
        "generate-responses",
        "multi-steer",
        "modify-weights",
        "optimize-steering",
        "verify-steering",
        "discover-steering",
        "find-best-method",
    ];
    if !allowed.contains(&operation[0].as_str()) {
        return Err(format!(
            "{} is not one of the product commands instrumented by record_representation_operation",
            operation[0]
        ));
    }
    if operation.iter().any(|x| x == "--help") {
        return Err("--help exits before the product handler and is not first success".into());
    }
    let root = Path::new("/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/wisent");
    let temp = common::scratch("probierz-wisent-first-use")?;
    let home = temp.join("home");
    let py_path = format!(
        "{}:{}",
        root.display(),
        std::env::var("PYTHONPATH").unwrap_or_default()
    );
    let env = common::env_map([
        ("HOME", home.to_string_lossy().as_ref()),
        ("PYTHONPATH", py_path.as_str()),
    ]);
    let state_path = home.join(".wisent/onboarding/first-use.json");
    let result = (|| {
        let shown = invoke(
            &python,
            &["onboarding".into()],
            root,
            &env,
            Duration::from_secs(120),
        )?;
        if !shown
            .stdout
            .to_lowercase()
            .contains("engineer a representation, end to end")
        {
            return Err(format!(
                "expected Engineer a representation, end to end: {}",
                shown.stdout
            ));
        }
        let mut state = common::read_json(&state_path)?;
        let attempt = state["progress"]["attempt_id"]
            .as_str()
            .ok_or("progress attempt_id is required")?
            .to_string();
        if state["bundle"]["definition"]["journey_version"] != "2026-08-04.1"
            || state["bundle"]["definition"]["first_success_fact"]
                != "representation_operation_completed"
            || state["progress"]["current_screen_id"] != "welcome"
            || state["progress"]["status"] != "in_progress"
        {
            return Err(format!("unexpected fresh state: {state}"));
        }
        invoke(
            &python,
            &["onboarding".into(), "continue".into()],
            root,
            &env,
            Duration::from_secs(120),
        )?;
        state = common::read_json(&state_path)?;
        if state["progress"]["attempt_id"] != attempt
            || state["progress"]["current_screen_id"] != "workflow"
        {
            return Err("a second process must resume the same attempt".into());
        }
        invoke(
            &python,
            &["onboarding".into(), "continue".into()],
            root,
            &env,
            Duration::from_secs(120),
        )?;
        let blocked = invoke(
            &python,
            &["onboarding".into(), "continue".into()],
            root,
            &env,
            Duration::from_secs(120),
        )?;
        if !blocked
            .stdout
            .to_lowercase()
            .contains("2/3 steps complete (in_progress)")
        {
            return Err(format!(
                "expected 2/3 steps complete (in_progress): {}",
                blocked.stdout
            ));
        }
        state = common::read_json(&state_path)?;
        if state["progress"]["status"] != "in_progress" {
            return Err("navigation alone completed first use".into());
        }
        invoke(
            &python,
            &operation,
            Path::new(&workdir),
            &env,
            Duration::from_secs(3600),
        )?;
        state = common::read_json(&state_path)?;
        if state["progress"]["attempt_id"] != attempt
            || state["progress"]["status"] != "completed"
            || state["progress"]["evidence_revision"].is_null()
        {
            return Err("representation operation did not complete onboarding".into());
        }
        let events = state["pending_events"]
            .as_array()
            .ok_or("pending_events missing")?;
        let success = events
            .iter()
            .find(|e| e["event_name"] == "onboarding_first_success_observed")
            .ok_or("canonical first-success event must be retained offline")?;
        if success["properties"]["command"] != operation[0] {
            return Err("first-success event names the wrong command".into());
        }
        common::write_json(
            &context
                .artifacts
                .join("wisent-onboarding-first-use.trace.json"),
            &json!({"schemaVersion":1,"productId":"wisent","journeyId":"first-use","journeyVersion":"2026-08-04.1","journeyVersionId":state["bundle"]["journey_version_id"],"sourceRevision":state["bundle"]["definition"]["source_revision"],"firstSuccessFact":"representation_operation_completed","attemptId":attempt,"evidenceRevision":state["progress"]["evidence_revision"],"completionEventId":success["event_id"],"observation":{"command":operation[0]}}),
        )?;
        let final_view = invoke(
            &python,
            &["onboarding".into()],
            root,
            &env,
            Duration::from_secs(120),
        )?;
        if !final_view
            .stdout
            .to_lowercase()
            .contains("first success observed: a representation operation completed successfully")
        {
            return Err(format!(
                "missing completed first success: {}",
                final_view.stdout
            ));
        }
        Ok(())
    })();
    common::remove(&temp);
    result
}
