use crate::{
    specs::{self, tui::common},
    tui::{Spawn, Terminal},
};
use std::time::Duration;

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = common::required(
        context,
        "TUI_CMD",
        "TUI_CMD is required: provide the released Jeden executable",
    )?;
    for (name, detail) in [
        (
            "BRAMA_URL",
            "provide the externally provisioned Brama router URL",
        ),
        (
            "WISENT_APP_AGENT_ID",
            "provide the externally provisioned Jeden workload identity",
        ),
        (
            "WISENT_APP_AGENT_AUTH_SECRET",
            "provide the workload signing credential outside Probierz",
        ),
        (
            "JEDEN_MODEL",
            "provide a real model coordinate available to that workload",
        ),
    ] {
        common::required(context, name, &format!("{name} is required: {detail}"))?;
    }
    let temp = common::scratch("probierz-jeden-first-use")?;
    let make = || {
        Terminal::spawn(
            Spawn::new(&binary)
                .cwd(&temp)
                .size(120, 40)
                .env("HOME", temp.to_string_lossy())
                .env("XDG_STATE_HOME", temp.join("state").to_string_lossy())
                .env("XDG_CONFIG_HOME", temp.join("config").to_string_lossy())
                .env("XDG_CACHE_HOME", temp.join("cache").to_string_lossy())
                .env("USER", "probierz-jeden-first-use"),
        )
        .map_err(|e| e.detail)
    };
    let result = (|| {
        let mut app = make()?;
        let fresh = app
            .wait_for(
                "Turn a coding task into a verified change",
                Duration::from_secs(30),
                false,
            )
            .map_err(|e| e.detail)?;
        common::excludes(
            &fresh,
            "Jeden first-use complete",
            format!("unexpected Jeden first-use complete: {fresh}"),
        )?;
        app.key("enter").map_err(|e| e.detail)?;
        app.wait_for("You stay in control", Duration::from_secs(15), false)
            .map_err(|e| e.detail)?;
        app.close().map_err(|e| e.detail)?;
        let mut app = make()?;
        let resumed = app
            .wait_for("You stay in control", Duration::from_secs(30), false)
            .map_err(|e| e.detail)?;
        common::excludes(
            &resumed,
            "Jeden first-use complete",
            format!("unexpected Jeden first-use complete: {resumed}"),
        )?;
        app.key("enter").map_err(|e| e.detail)?;
        app.wait_for("Give Jeden one real task", Duration::from_secs(15), false)
            .map_err(|e| e.detail)?;
        app.key("esc").map_err(|e| e.detail)?;
        app.send("Respond exactly: OK").map_err(|e| e.detail)?;
        app.key("enter").map_err(|e| e.detail)?;
        let turn = app
            .wait_for("OK", Duration::from_secs(180), true)
            .map_err(|e| e.detail)?;
        common::contains(
            &turn,
            "wisent",
            format!("expected wisent followed by OK: {turn}"),
        )?;
        app.send("/onboarding").map_err(|e| e.detail)?;
        app.key("enter").map_err(|e| e.detail)?;
        let completed = app
            .wait_for("Jeden first-use complete", Duration::from_secs(30), false)
            .map_err(|e| e.detail)?;
        common::contains(
            &completed,
            "Replay the product guide",
            format!("expected Replay the product guide: {completed}"),
        )?;
        common::excludes(
            &completed,
            "Configure model access",
            format!("unexpected Configure model access: {completed}"),
        )?;
        app.close().map_err(|e| e.detail)?;
        Ok(())
    })();
    common::remove(&temp);
    result
}
