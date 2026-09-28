//! What the keys do across screens: cross the panes, walk a list, choose a
//! model and land back with the choice applied, and a destructive row that
//! asks first. A layout that looks right and answers to nothing passes every
//! static check. Each journey runs jeden in tmux (a real emulator), keeps the
//! pane after every step as a Markdown trace, and fails naming the steps
//! that did nothing.

use std::fs;
use std::path::Path;
use std::time::Duration;

use regex::Regex;

use crate::specs::tui::jeden::screens::tmux::Tmux;
use crate::specs::{self, tui::common};

/// A step gets fifteen seconds to settle; walking the list stops after eight rows.
const STEP_SECONDS: u64 = 15;
const PICK_STEPS: usize = 8;

fn row<'a>(pane: &'a str, marker: char) -> &'a str {
    pane.lines()
        .find(|line| line.contains(marker))
        .map(str::trim)
        .unwrap_or_default()
}

/// The row the model cursor (`›`) is on, and the row the brand cursor (`❯`) is on.
fn cursor(pane: &str) -> &str {
    row(pane, '›')
}
fn brand(pane: &str) -> &str {
    row(pane, '❯')
}

struct Step {
    label: &'static str,
    ok: bool,
    pane: String,
}

/// Run jeden in tmux on a warm sandbox home with Brama reachable.
fn session(
    context: &specs::Context,
    body: impl FnOnce(&Tmux, &Path) -> Result<(), String>,
) -> Result<(), String> {
    crate::specs::tui::jeden::cli::network::brama(context)?;
    let binary = context
        .optional("TUI_CMD")
        .unwrap_or_else(|| "jeden".to_string());
    let home = crate::specs::tui::jeden::sandbox::home(true, true)?;
    let cwd = common::scratch("probierz-keys")?;
    let outcome =
        Tmux::start(&format!("{binary} --cwd {}", cwd.display()), Some(&home)).and_then(|tmux| {
            tmux.settled(Duration::from_secs(STEP_SECONDS))?;
            body(&tmux, &home)
        });
    common::remove(&home);
    common::remove(&cwd);
    outcome
}

/// Keep the journey on disk and fail naming the steps that did nothing.
fn verdict(context: &specs::Context, name: &str, steps: &[Step]) -> Result<(), String> {
    let body: Vec<String> = steps
        .iter()
        .map(|step| {
            format!(
                "## {} — {}\n\n```\n{}\n```\n",
                if step.ok { "ok" } else { "FAILED" },
                step.label,
                step.pane
            )
        })
        .collect();
    let path = context.artifacts.join(format!("journey-{name}.md"));
    fs::write(&path, format!("# journey: {name}\n\n{}", body.join("\n")))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    context.media_typed("trace", path, "text/markdown");
    let failed: Vec<&str> = steps
        .iter()
        .filter(|step| !step.ok)
        .map(|step| step.label)
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("these steps did nothing: {}", failed.join(", ")))
    }
}

/// jeden-pane-crossing: arrows cross the panes and the cursor follows.
pub fn pane_crossing(context: &specs::Context) -> Result<(), String> {
    session(context, |tmux, _| {
        let settle = || tmux.settled(Duration::from_secs(STEP_SECONDS));
        tmux.submit("/model")?;
        let (opened, _) =
            tmux.until("the model hub", Duration::from_secs(STEP_SECONDS), |pane| {
                pane.contains("Select model route")
            })?;
        let mut steps = vec![Step {
            label: "open the model hub",
            ok: opened.contains('┬'),
            pane: opened.clone(),
        }];
        let mut before = opened;
        let moves: [(&'static str, &str, fn(&str, &str) -> bool); 5] = [
            ("brands hold the keyboard first", "Down", |after, before| {
                brand(after) != brand(before) && cursor(after).is_empty()
            }),
            (
                "→ hands the keyboard to the models",
                "Right",
                |after, _| !cursor(after).is_empty() && brand(after).is_empty(),
            ),
            (
                "↓ walks the models, not the brands",
                "Down",
                |after, before| cursor(after) != cursor(before) && brand(after).is_empty(),
            ),
            ("← hands it back to the brands", "Left", |after, _| {
                !brand(after).is_empty() && cursor(after).is_empty()
            }),
            (
                "a different brand narrows the model pane",
                "Down",
                |after, before| brand(after) != brand(before),
            ),
        ];
        for (label, key, holds) in moves {
            tmux.key(key)?;
            let after = settle()?;
            steps.push(Step {
                label,
                ok: holds(&after, &before),
                pane: after.clone(),
            });
            before = after;
        }
        verdict(context, "pane-crossing", &steps)
    })
}

/// jeden-selection-applies: choosing a model closes the view, shows it on the status line and persists it.
pub fn selection_applies(context: &specs::Context) -> Result<(), String> {
    let model_id = Regex::new(r"(?i)[a-z\d.-]+/[a-z\d.:-]+").map_err(|error| error.to_string())?;
    session(context, |tmux, home| {
        let settle = || tmux.settled(Duration::from_secs(STEP_SECONDS));
        tmux.submit("/model")?;
        tmux.until("the model hub", Duration::from_secs(STEP_SECONDS), |pane| {
            pane.contains("Select model route")
        })?;
        tmux.key("Right")?;
        let mut pane = settle()?;
        // The first rows are the automatic routes and the active model, which
        // cannot be chosen again: walk to a row that names another route.
        let mut chosen = None;
        for _ in 0..PICK_STEPS {
            let row = cursor(&pane);
            if let Some(found) = model_id.find(row).filter(|_| !row.contains("[ACTIVE]")) {
                chosen = Some(found.as_str().to_string());
                break;
            }
            tmux.key("Down")?;
            pane = settle()?;
        }
        let chosen = chosen.ok_or("no selectable model route under the cursor")?;
        tmux.key("Enter")?;
        tmux.until(
            "the picker to close",
            Duration::from_secs(STEP_SECONDS),
            |pane| !pane.contains("Esc close"),
        )?;
        let after = settle()?;
        let config = fs::read_to_string(home.join(".jeden/config.yml")).unwrap_or_default();
        if !after.contains(&chosen) || !config.contains(&chosen) {
            return Err(format!(
                "choosing {chosen} did not switch the route: status line {}, config.yml {}",
                after.contains(&chosen),
                config.contains(&chosen)
            ));
        }
        Ok(())
    })
}

/// jeden-confirm-guards: a destructive row asks before it acts, and Esc means no.
pub fn confirm_guards(context: &specs::Context) -> Result<(), String> {
    let asking = Regex::new(r"(?i)confirm|cancel").map_err(|error| error.to_string())?;
    let confirm_panel = Regex::new(r"(?i)confirm .*action").map_err(|error| error.to_string())?;
    session(context, |tmux, _| {
        let settle = || tmux.settled(Duration::from_secs(STEP_SECONDS));
        tmux.submit("/usage")?;
        let (opened, _) = tmux.until(
            "the usage picker",
            Duration::from_secs(STEP_SECONDS),
            |pane| pane.contains("Esc close"),
        )?;
        let mut steps = vec![Step {
            label: "open usage",
            ok: true,
            pane: opened,
        }];
        tmux.type_text("reset")?;
        let searched = settle()?;
        steps.push(Step {
            label: "search reaches the reset row",
            ok: searched.to_lowercase().contains("reset"),
            pane: searched,
        });
        tmux.key("Enter")?;
        let asked = settle()?;
        steps.push(Step {
            label: "Enter opens a confirm panel instead of resetting",
            ok: asking.is_match(&asked),
            pane: asked,
        });
        tmux.key("Escape")?;
        let cancelled = settle()?;
        steps.push(Step {
            label: "Esc cancels it",
            ok: !confirm_panel.is_match(&cancelled),
            pane: cancelled,
        });
        verdict(context, "destructive-guard", &steps)
    })
}
