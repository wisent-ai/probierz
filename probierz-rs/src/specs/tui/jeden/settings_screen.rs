use crate::{
    specs::{self, tui::common},
    tui::{Spawn, Terminal},
};
use std::time::Duration;

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = context
        .optional("TUI_CMD")
        .unwrap_or_else(|| "jeden".to_string());
    let mut app = Terminal::spawn(Spawn::new(binary)).map_err(|e| e.detail)?;
    let result = (|| {
        let initial = app
            .wait_for("Welcome back!", Duration::from_secs(30), false)
            .map_err(|e| e.detail)?;
        common::contains(
            &initial,
            "Wisent Agent",
            format!("expected the initial screen to show Wisent Agent: {initial}"),
        )?;
        app.send("/settings").map_err(|e| e.detail)?;
        app.key("enter").map_err(|e| e.detail)?;
        let settings = app
            .wait_for("── secrets (", Duration::from_secs(30), false)
            .map_err(|e| e.detail)?;
        for group in ["tools", "commands", "startup", "secrets"] {
            common::contains(
                &settings,
                &format!("── {group} ("),
                format!("expected the settings screen to show the {group} group header"),
            )?;
        }
        Ok(())
    })();
    let _ = app.close();
    result
}
