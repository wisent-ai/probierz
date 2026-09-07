use std::time::Duration;

use crate::{
    specs::{self, tui::common},
    tui::{Spawn, Terminal},
};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = context.required("TUI_CMD", "provide the released Brama executable and an externally provisioned real model route/workload")?;
    let model = context.required("BRAMA_ONBOARDING_MODEL", "provide the released Brama executable and an externally provisioned real model route/workload")?;
    let agent = context.required("BRAMA_ONBOARDING_AGENT_ID", "provide the released Brama executable and an externally provisioned real model route/workload")?;
    let temp = common::scratch("probierz-brama-first-use")?;
    let state = temp.join("state");
    let brama_state = temp.join("brama-state");
    let catalog = temp.join("model-catalog.json");
    let perf = temp.join("perf.json");
    let spawn = |cost: bool| {
        let mut spec = Spawn::new(&binary)
            .args(["onboard", "--model", &model, "--agent-id", &agent])
            .env("XDG_STATE_HOME", state.to_string_lossy())
            .env("BRAMA_STATE_DIR", brama_state.to_string_lossy())
            .env("BRAMA_MODEL_CATALOG_CACHE", catalog.to_string_lossy())
            .env("BRAMA_PERF_PATH", perf.to_string_lossy());
        if cost {
            spec = spec.arg("--allow-provider-cost");
        }
        Terminal::spawn(spec).map_err(|e| e.detail)
    };
    let outcome = (|| {
        let first = spawn(false)?;
        let fresh = first
            .wait_for(
                "No provider request was sent and onboarding remains in progress.",
                Duration::from_secs(30),
                true,
            )
            .map_err(|e| e.detail)?;
        common::contains(
            &fresh,
            "Receive one real model response",
            format!("expected Receive one real model response: {fresh}"),
        )?;
        common::excludes(
            &fresh,
            "First-use complete:",
            format!("unexpected First-use complete:: {fresh}"),
        )?;
        first.close().map_err(|e| e.detail)?;
        let completed_app = spawn(true)?;
        let completed = completed_app.wait_for("First-use complete: Brama observed model_response_received from the real response above.", Duration::from_secs(180), true).map_err(|e| e.detail)?;
        for needle in [
            "Sending one billable model request through route",
            "Model: ",
            "Response: ",
            "Tokens: ",
        ] {
            common::contains(
                &completed,
                needle,
                format!("expected {needle}: {completed}"),
            )?;
        }
        common::contains(
            &completed,
            " in / ",
            format!("expected Tokens: <number> in / <number> out: {completed}"),
        )?;
        common::excludes(
            &completed,
            "Model request failed:",
            format!("unexpected Model request failed:: {completed}"),
        )?;
        completed_app.close().map_err(|e| e.detail)?;
        Ok(())
    })();
    common::remove(&temp);
    outcome
}
