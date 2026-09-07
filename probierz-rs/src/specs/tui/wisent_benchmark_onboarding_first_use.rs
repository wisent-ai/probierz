use crate::specs::{self, tui::common};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, time::Duration};
fn py(
    python: &str,
    module: &str,
    args: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    input: Option<&str>,
    timeout: Duration,
) -> Result<(), String> {
    let mut argv = vec!["-m".into(), module.into()];
    argv.extend_from_slice(args);
    let out = common::run(
        python,
        &argv,
        Some(cwd),
        env,
        &[
            "STADO_INTEGRATION_API_URL",
            "WISENT_BENCHMARK_STADO_INTEGRATION_TOKEN",
        ],
        input,
        timeout,
    )?;
    if !out.status.success() {
        return Err(format!(
            "{module} exited {:?}\nstdout:\n{}\nstderr:\n{}",
            out.code(),
            out.stdout,
            out.stderr
        ));
    }
    Ok(())
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let req = |n: &str| {
        common::required(context,n,&format!("{n} is required; Probierz will not invent model, dataset, interpreter, or workspace coordinates"))
    };
    let python = req("PROBIERZ_WISENT_BENCHMARK_PYTHON")?;
    let workdir = req("PROBIERZ_WISENT_BENCHMARK_WORKDIR")?;
    let text = req("PROBIERZ_WISENT_BENCHMARK_ARGS_JSON")?;
    let mut args: Vec<String> = serde_json::from_str(&text).map_err(|e| {
        format!("PROBIERZ_WISENT_BENCHMARK_ARGS_JSON must be a JSON argv array: {e}")
    })?;
    if args.iter().any(|x| x == "--output") {
        return Err(
            "the scenario owns --output so it can bind success to the produced result".into(),
        );
    }
    let mi = args.iter().position(|x| x == "--model");
    if mi.is_none() || args.get(mi.unwrap() + 1).is_none() {
        return Err("benchmark argv must select a real --model".into());
    }
    let bi = args
        .iter()
        .position(|x| x == "--benchmark")
        .ok_or("benchmark argv must select a dataset with --benchmark")?;
    if !args
        .get(bi + 1)
        .is_some_and(|x| ["livecodebench", "truthfulqa", "dna", "all"].contains(&x.as_str()))
    {
        return Err("the empty example dataset cannot be presented as first success".into());
    }
    let root =
        Path::new("/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/wisent-benchmark");
    let temp = common::scratch("probierz-wisent-benchmark-first-use")?;
    let state_home = temp.join("state");
    let result_path = temp.join("benchmark-result.json");
    let env = common::env_map([
        ("XDG_STATE_HOME", state_home.to_string_lossy().as_ref()),
        (
            "WISENT_BENCHMARK_ONBOARDING_SUBJECT",
            "probierz-isolated-wisent-benchmark-first-use",
        ),
        (
            "PYTHONPATH",
            format!(
                "{}:{}",
                root.display(),
                std::env::var("PYTHONPATH").unwrap_or_default()
            )
            .as_str(),
        ),
    ]);
    let state_path = state_home.join("wisent-benchmark/onboarding.json");
    let result = (|| {
        py(
            &python,
            "wisent_benchmark.onboarding",
            &[],
            root,
            &env,
            Some("\n"),
            Duration::from_secs(120),
        )?;
        let mut state = common::read_json(&state_path)?;
        let progress = state["progress"]
            .as_object()
            .and_then(|m| m.values().next())
            .ok_or("isolated subject must have exactly one durable progress record")?;
        let attempt = progress["attempt_id"]
            .as_str()
            .ok_or("attempt id missing")?
            .to_string();
        if state["bundle"]["definition"]["journey_version"] != "2026-08-04.1"
            || progress["current_screen_id"] != "inputs"
            || progress["status"] != "in_progress"
        {
            return Err(format!("unexpected fresh benchmark state: {state}"));
        }
        py(
            &python,
            "wisent_benchmark.onboarding",
            &[],
            root,
            &env,
            Some("\n"),
            Duration::from_secs(120),
        )?;
        state = common::read_json(&state_path)?;
        if state["progress"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()["current_screen_id"]
            != "metrics"
        {
            return Err("new process did not resume at metrics".into());
        }
        py(
            &python,
            "wisent_benchmark.onboarding",
            &[],
            root,
            &env,
            Some("\n"),
            Duration::from_secs(120),
        )?;
        py(
            &python,
            "wisent_benchmark.onboarding",
            &[],
            root,
            &env,
            None,
            Duration::from_secs(120),
        )?;
        state = common::read_json(&state_path)?;
        if state["progress"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()["status"]
            != "in_progress"
        {
            return Err("navigation alone completed benchmark first use".into());
        }
        args.extend([
            "--output".into(),
            result_path.to_string_lossy().into_owned(),
        ]);
        py(
            &python,
            "wisent_benchmark.bench",
            &args,
            Path::new(&workdir),
            &env,
            None,
            Duration::from_secs(1800),
        )?;
        let benchmark = common::read_json(&result_path)?;
        if !benchmark["total_problems"].as_i64().is_some_and(|n| n > 0)
            || !benchmark["problem_results"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
        {
            return Err("a real benchmark result must contain problems and problem results".into());
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
            || progress["evidence"]["benchmark_result_observed"] != true
        {
            return Err("benchmark result did not complete onboarding".into());
        }
        let success = state["events"]
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|e| e["event_name"] == "onboarding_first_success_observed")
            })
            .ok_or("canonical first-success event must be retained or delivered")?;
        common::write_json(
            &context
                .artifacts
                .join("wisent-benchmark-onboarding-first-use.trace.json"),
            &json!({"schemaVersion":1,"productId":"wisent-benchmark","journeyId":"first-use","journeyVersion":"2026-08-04.1","journeyVersionId":state["bundle"]["journey_version_id"],"sourceRevision":state["bundle"]["definition"]["source_revision"],"firstSuccessFact":"benchmark_result_observed","attemptId":attempt,"evidenceRevision":progress["evidence_revision"],"completionEventId":success["event_id"],"observation":{"totalProblems":benchmark["total_problems"],"evaluated":benchmark["evaluated"],"problemResultCount":benchmark["problem_results"].as_array().unwrap().len()}}),
        )
    })();
    common::remove(&temp);
    result
}
