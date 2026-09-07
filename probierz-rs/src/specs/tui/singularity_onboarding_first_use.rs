use crate::{
    specs::{self, tui::common},
    tui::{Spawn, Terminal},
};
use std::{fs, path::Path, time::Duration};

fn copy_dir(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let to = target.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_dir(&entry.path(), &to)?
        } else {
            fs::copy(entry.path(), to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn cli(
    python: &str,
    args: &[&str],
    marker: &str,
    root: &Path,
    temp: &Path,
) -> Result<String, String> {
    let app = Terminal::spawn(
        Spawn::new(python)
            .args(["-m", "singularity.autonomous_agent"])
            .args(args.iter().copied())
            .cwd(root)
            .env("HOME", temp.join("home").to_string_lossy())
            .env("XDG_STATE_HOME", temp.join("state").to_string_lossy())
            .env("PYTHONPATH", root.to_string_lossy())
            .env(
                "PYTHONPYCACHEPREFIX",
                temp.join("pycache").to_string_lossy(),
            )
            .env(
                "SINGULARITY_ONBOARDING_STATE_PATH",
                temp.join("state/onboarding.json").to_string_lossy(),
            )
            .env(
                "SINGULARITY_ONBOARDING_SUBJECT",
                "probierz-isolated-first-use",
            )
            .env("AGENT_NAME", "Probierz Agent")
            .env("AGENT_TICKER", "PROBIERZ")
            .env("AGENT_TYPE", "general")
            .env("STARTING_BALANCE", "1"),
    )
    .map_err(|e| e.detail)?;
    app.wait_for(marker, Duration::from_secs(30), true)
        .map_err(|e| e.detail)?;
    let log = app.full_log();
    app.close().map_err(|e| e.detail)?;
    Ok(log)
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    for (n, d) in [
        (
            "STADO_MODEL_ROUTER_URL",
            "provide the externally provisioned Stado model-router URL",
        ),
        (
            "SINGULARITY_MODEL_ROUTER_TOKEN",
            "provide the Singularity-scoped router bearer outside Probierz",
        ),
        (
            "STADO_TEXT_MODEL",
            "provide a real model coordinate available to that bearer",
        ),
    ] {
        common::required(context, n, &format!("{n} is required: {d}"))?;
    }
    let python = context
        .optional("SINGULARITY_PYTHON")
        .unwrap_or_else(|| "python3".into());
    let temp = common::scratch("probierz-singularity-first-use")?;
    let root = temp.join("subject");
    fs::create_dir_all(temp.join("home")).map_err(|e| e.to_string())?;
    copy_dir(
        Path::new(
            "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/singularity/singularity",
        ),
        &root.join("singularity"),
    )?;
    let result = (|| {
        let fresh = cli(
            &python,
            &["onboarding"],
            "Meet the agent loop",
            &root,
            &temp,
        )?;
        common::excludes(
            &fresh,
            "Journey complete:",
            format!("unexpected Journey complete:: {fresh}"),
        )?;
        let resumed = cli(
            &python,
            &["onboarding", "next"],
            "Tools are the boundary",
            &root,
            &temp,
        )?;
        common::excludes(
            &resumed,
            "Journey complete:",
            format!("unexpected Journey complete:: {resumed}"),
        )?;
        cli(
            &python,
            &["onboarding", "next"],
            "Results become evidence",
            &root,
            &temp,
        )?;
        let terminal = cli(
            &python,
            &["onboarding", "next"],
            "Observe one real loop result",
            &root,
            &temp,
        )?;
        common::contains(
            &terminal,
            "Next: run `singularity` and observe its RESULT line.",
            format!("missing Next instruction: {terminal}"),
        )?;
        common::excludes(
            &terminal,
            "Journey complete:",
            format!("unexpected Journey complete:: {terminal}"),
        )?;
        let agent = Terminal::spawn(
            Spawn::new(&python)
                .args(["-m", "singularity.autonomous_agent"])
                .cwd(&root)
                .size(120, 40)
                .env("HOME", temp.join("home").to_string_lossy())
                .env("XDG_STATE_HOME", temp.join("state").to_string_lossy())
                .env("PYTHONPATH", root.to_string_lossy())
                .env(
                    "PYTHONPYCACHEPREFIX",
                    temp.join("pycache").to_string_lossy(),
                )
                .env(
                    "SINGULARITY_ONBOARDING_STATE_PATH",
                    temp.join("state/onboarding.json").to_string_lossy(),
                )
                .env(
                    "SINGULARITY_ONBOARDING_SUBJECT",
                    "probierz-isolated-first-use",
                )
                .env("AGENT_NAME", "Probierz Agent")
                .env("AGENT_TICKER", "PROBIERZ")
                .env("AGENT_TYPE", "general")
                .env("STARTING_BALANCE", "1"),
        )
        .map_err(|e| e.detail)?;
        let log = agent
            .wait_for("[RESULT]", Duration::from_secs(180), true)
            .map_err(|e| e.detail)?;
        common::contains(&log, "[DO]", format!("expected [DO]: {log}"))?;
        common::contains(&log, "[RESULT]", format!("expected [RESULT] JSON: {log}"))?;
        agent.close().map_err(|e| e.detail)?;
        let completed = cli(
            &python,
            &["onboarding", "status"],
            "Journey complete: a real agent loop result was observed.",
            &root,
            &temp,
        )?;
        common::contains(
            &completed,
            "Observe one real loop result",
            format!("missing Observe one real loop result: {completed}"),
        )?;
        Ok(())
    })();
    common::remove(&temp);
    result
}
