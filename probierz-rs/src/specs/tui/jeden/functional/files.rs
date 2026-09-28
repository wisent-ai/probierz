//! Commands whose effect is a file: jeden's config, its rules, its collab
//! relay, a registered marketplace catalogue, and the secret /token must not
//! print. Each journey reads the file jeden wrote, not only the screen.

use std::fs;
use std::path::PathBuf;

use super::{journey, run};
use crate::specs::tui::jeden::{sandbox, views::discovery::fixture};
use crate::specs::{self, tui::common};

/// jeden-settings-write-through: /settings set writes the value to config.yml on disk.
pub fn settings_write_through(context: &specs::Context) -> Result<(), String> {
    journey(context, |app, home, _| {
        let answer = run(app, "/settings set tools.approvalMode always-ask")?;
        let lower = answer.to_lowercase();
        if !lower.contains("config.yml") && !lower.contains("config.yaml") {
            return Err(format!(
                "/settings set did not name the file it wrote:\n{answer}"
            ));
        }
        let config = home.join(".jeden/config.yml");
        let written = fs::read_to_string(&config).unwrap_or_default();
        common::contains(
            &written,
            "always-ask",
            format!(
                "/settings set reported success but {} does not carry the value",
                config.display()
            ),
        )
    })
}

/// jeden-omfg-persists: /omfg writes the rule into the workspace rules file.
pub fn omfg(context: &specs::Context) -> Result<(), String> {
    let rule = super::marker("probe rule ");
    journey(context, |app, _, cwd| {
        run(app, &format!("/omfg {rule}"))?;
        let rules = cwd.join(".jeden/rules.jsonl");
        let stored = fs::read_to_string(&rules).unwrap_or_default();
        common::contains(
            &stored,
            &rule,
            format!(
                "/omfg accepted the rule but {} does not contain it",
                rules.display()
            ),
        )
    })
}

/// jeden-token-redacted: /token never prints the agent secret in full.
pub fn token_redacted(context: &specs::Context) -> Result<(), String> {
    let env = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".jeden/.env");
    let text = fs::read_to_string(&env).map_err(|error| {
        format!(
            "{} is required to compare /token against: {error}",
            env.display()
        )
    })?;
    let secret = text
        .lines()
        .find_map(|line| line.strip_prefix("WISENT_APP_AGENT_AUTH_SECRET="))
        .map(|value| {
            value
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string()
        })
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "WISENT_APP_AGENT_AUTH_SECRET is required in {} to compare /token against",
                env.display()
            )
        })?;
    journey(context, |app, _, _| {
        let screen = run(app, "/token")?;
        common::excludes(
            &screen,
            &secret,
            "/token printed the raw agent secret into the transcript the model reads",
        )
    })
}

/// jeden-collab-relay: /collab start opens a durable relay and /collab stop closes it.
pub fn collab(context: &specs::Context) -> Result<(), String> {
    journey(context, |app, _, cwd| {
        let started = run(app, "/collab start")?;
        common::contains(
            &started.to_lowercase(),
            "collab-relay.jsonl",
            format!("/collab start named no relay:\n{started}"),
        )?;
        // The relay is a file: a host that "started" without writing its own
        // start event started nothing.
        let relay = cwd.join(".jeden/collab-relay.jsonl");
        let events = fs::read_to_string(&relay).unwrap_or_default();
        common::contains(
            &events,
            "host-start",
            format!("{} holds no host-start event", relay.display()),
        )?;
        let hosting = run(app, "/collab status")?;
        common::contains(
            &hosting.to_lowercase(),
            "collab host:",
            format!("/collab status does not report hosting:\n{hosting}"),
        )?;
        let stopped = run(app, "/collab stop")?;
        common::contains(
            &stopped.to_lowercase(),
            "hosting stopped",
            format!("/collab stop did not stop hosting:\n{stopped}"),
        )?;
        let off = run(app, "/collab status")?;
        common::contains(
            &off.to_lowercase(),
            "collab off",
            format!("/collab status after stop does not report collab off:\n{off}"),
        )
    })
}

/// jeden-marketplace-source: /marketplace add registers a local catalogue and lists its plugins.
pub fn marketplace(context: &specs::Context) -> Result<(), String> {
    journey(context, |app, _, cwd| {
        let source = cwd.join("probe-market");
        sandbox::copy_tree(&fixture(context, "probe-market"), &source)?;
        let added = run(app, &format!("/marketplace add {}", source.display()))?;
        common::contains(
            &added.to_lowercase(),
            "added marketplace source",
            format!(
                "/marketplace add did not accept {}:\n{added}",
                source.display()
            ),
        )?;
        let listed = run(app, "/marketplace")?;
        common::contains(
            &listed,
            "probe-plugin",
            "/marketplace registered the source but its plugin is not offered in the view",
        )
    })
}
