//! Screen semantics, run the same way against jeden and against omp as the
//! reference control: overlays fit the viewport, replace each other instead
//! of appending to the transcript, keep transcript growth bounded, and the
//! model picker is a two-pane view. A failure on the omp side means the
//! journey itself is miscalibrated, so it is reported as such.

use std::path::PathBuf;
use std::time::Duration;

use super::constants::{
    FRAME_BUDGET_AFTER_TWO_OVERLAYS, MIN_TWO_PANE_ROWS, READY_SECONDS, VIEW_SECONDS,
};
use super::geometry::{box_frames, pane_geometry};
use super::tmux::Tmux;
use crate::specs::{self, tui::common};

/// One application the journeys describe through the same fields.
struct Profile {
    name: &'static str,
    command: String,
    home: Option<PathBuf>,
    cwd: Option<PathBuf>,
    model_command: &'static str,
    model_title: fn(&str) -> bool,
    settings_title: fn(&str) -> bool,
}

const FOOTER: &str = "Esc close";

fn jeden(context: &specs::Context) -> Result<Profile, String> {
    super::super::cli::network::brama(context)?;
    let binary = context
        .optional("TUI_CMD")
        .unwrap_or_else(|| "jeden".to_string());
    let cwd = common::scratch("probierz-screen")?;
    Ok(Profile {
        name: "jeden",
        command: format!("{binary} --cwd {}", cwd.display()),
        home: Some(super::super::sandbox::home(true, true)?),
        cwd: Some(cwd),
        // The bare command, because that is what a user types.
        model_command: "/model",
        model_title: |pane| pane.contains("Select model route"),
        settings_title: |pane| pane.contains("Jeden settings"),
    })
}

/// omp, when OMP_BIN or `omp` on PATH answers --version.
fn omp(context: &specs::Context) -> Option<Profile> {
    let binary = context
        .optional("OMP_BIN")
        .unwrap_or_else(|| "omp".to_string());
    let probe = common::run(
        &binary,
        &["--version".into()],
        None,
        &Default::default(),
        &[],
        None,
        Duration::from_secs(READY_SECONDS),
    );
    probe.ok().filter(|output| output.code() == Some(0))?;
    Some(Profile {
        name: "omp",
        command: format!("{binary} --allow-home"),
        home: None,
        cwd: None,
        model_command: "/models",
        model_title: |pane| pane.contains("All available models") || pane.contains("Roles"),
        settings_title: |pane| pane.contains("Appearance"),
    })
}

/// Run `check` against jeden, then against omp as the control when present.
fn both(
    context: &specs::Context,
    check: fn(&Tmux, &Profile) -> Result<(), String>,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for profile in [Some(jeden(context)?), omp(context)].into_iter().flatten() {
        let outcome = Tmux::start(&profile.command, profile.home.as_deref()).and_then(|session| {
            session.settled(Duration::from_secs(READY_SECONDS))?;
            check(&session, &profile)
        });
        for scratch in [&profile.home, &profile.cwd].into_iter().flatten() {
            common::remove(scratch);
        }
        if let Err(error) = outcome {
            let label = if profile.name == "omp" {
                "omp (the control; the journey is miscalibrated)"
            } else {
                "jeden"
            };
            failures.push(format!("{label}: {error}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn open_model(session: &Tmux, profile: &Profile) -> Result<String, String> {
    session.submit(profile.model_command)?;
    session
        .until(
            "the model view",
            Duration::from_secs(VIEW_SECONDS),
            profile.model_title,
        )
        .map(|(pane, _)| pane)
}

fn open_both(session: &Tmux, profile: &Profile) -> Result<String, String> {
    open_model(session, profile)?;
    session.key("Escape")?;
    session.submit("/settings")?;
    session.until(
        "the settings view",
        Duration::from_secs(VIEW_SECONDS),
        profile.settings_title,
    )?;
    session.history()
}

/// jeden-screen-geometry: the model overlay keeps its title and footer inside the viewport.
pub fn geometry(context: &specs::Context) -> Result<(), String> {
    both(context, |session, profile| {
        let pane = open_model(session, profile)?;
        if !pane.contains(FOOTER) {
            return Err(
                "title and footer are not in the same frame: the view is taller than the viewport"
                    .into(),
            );
        }
        Ok(())
    })
}

/// jeden-screen-replacement: a new overlay replaces the previous one instead of appending.
pub fn replacement(context: &specs::Context) -> Result<(), String> {
    both(context, |session, profile| {
        let history = open_both(session, profile)?;
        if (profile.model_title)(&history) {
            return Err(
                "the model view is still in the scrollback after opening settings: overlays append"
                    .into(),
            );
        }
        Ok(())
    })
}

/// jeden-screen-transcript-budget: transcript growth stays bounded across overlays.
pub fn transcript_budget(context: &specs::Context) -> Result<(), String> {
    both(context, |session, profile| {
        let frames = box_frames(&open_both(session, profile)?);
        if frames > FRAME_BUDGET_AFTER_TWO_OVERLAYS {
            return Err(format!("{frames} boxed frames after two overlays (budget {FRAME_BUDGET_AFTER_TWO_OVERLAYS})"));
        }
        Ok(())
    })
}

/// jeden-screen-two-pane: the model view is a brands/models split.
pub fn two_pane(context: &specs::Context) -> Result<(), String> {
    both(context, |session, profile| {
        let found = pane_geometry(&open_model(session, profile)?);
        let split = found.joined
            && found.aligned_rows >= MIN_TWO_PANE_ROWS
            && found.dots >= MIN_TWO_PANE_ROWS
            && found.aligned_metrics >= MIN_TWO_PANE_ROWS;
        if split {
            return Ok(());
        }
        Err(format!(
            "not a brands/models split: joined {}, {} aligned rows, {} brand dots, {} aligned metric rows",
            found.joined, found.aligned_rows, found.dots, found.aligned_metrics
        ))
    })
}
