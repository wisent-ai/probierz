//! One command driven in a live jeden and measured: did the screen change,
//! how many frames it appended, does the current frame fit the pane, and for
//! a picker whether ↓ moves the cursor, an unmatchable search empties it and
//! Esc closes it. Enter is never pressed: confirming a row runs its command.
//! What the painted text means is left to the routed judge.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use super::constants::{ONE_ROW, PAINT_SECONDS, UNMATCHABLE_QUERY};
use crate::specs::tui::jeden::screens::geometry::{box_frames, pane_geometry};
use crate::specs::tui::jeden::screens::tmux::Tmux;

/// What a picker's interaction showed; `None` when the command painted no picker.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Outcome {
    Yes,
    No,
    OneRow,
}

pub(super) struct Probe {
    pub(super) command: String,
    pub(super) painted: bool,
    pub(super) paint_ms: u128,
    pub(super) frames: usize,
    pub(super) fits: bool,
    pub(super) picker: bool,
    pub(super) navigates: Option<Outcome>,
    pub(super) filters: Option<Outcome>,
    pub(super) closes: Option<Outcome>,
    /// Only what this command painted: jeden keeps earlier frames on screen.
    pub(super) new_paint: String,
}

/// A picker is on the keyboard when its chrome is on screen.
pub(super) fn picker_open(pane: &str) -> bool {
    pane.contains("Esc close") || pane.contains("Type to search")
}

/// Does the current (last) frame open and close inside the pane?
fn frame_fits(pane: &str) -> bool {
    let lines: Vec<&str> = pane.lines().collect();
    let tops: Vec<usize> = (0..lines.len())
        .filter(|index| lines[*index].contains('╭'))
        .collect();
    let bottoms: Vec<usize> = (0..lines.len())
        .filter(|index| lines[*index].contains('╰'))
        .collect();
    match tops.last() {
        None => true,
        Some(last) => tops.len() <= bottoms.len() && bottoms.iter().any(|index| index > last),
    }
}

/// Rows a picker offers, counted by their `[BADGE]`.
fn picker_rows(pane: &str) -> usize {
    pane.lines()
        .filter(|line| {
            line.split('[').skip(1).any(|rest| {
                rest.split_once(']').is_some_and(|(badge, _)| {
                    badge.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                        && badge.chars().all(|c| {
                            c.is_ascii_uppercase() || c.is_ascii_digit() || " _-".contains(c)
                        })
                })
            })
        })
        .count()
}

fn cursor_line(pane: &str) -> String {
    pane.lines()
        .find(|line| line.contains('›'))
        .unwrap_or_default()
        .to_string()
}

fn outcome(multi_row: bool, holds: bool) -> Option<Outcome> {
    Some(match (multi_row, holds) {
        (false, _) => Outcome::OneRow,
        (true, true) => Outcome::Yes,
        (true, false) => Outcome::No,
    })
}

pub(super) fn probe(tmux: &Tmux, command: &str) -> Result<Probe, String> {
    let settle = || tmux.settled(Duration::from_secs(PAINT_SECONDS));
    tmux.key("Escape")?;
    let before = settle()?;
    let frames_before = box_frames(&tmux.history()?);
    let started = Instant::now();
    tmux.submit(command)?;
    let painted = tmux
        .until(command, Duration::from_secs(PAINT_SECONDS), |pane| {
            pane != before
        })
        .is_ok();
    let paint_ms = started.elapsed().as_millis();
    let screen = settle()?;
    let seen: HashSet<&str> = before.lines().collect();
    let new_paint: Vec<&str> = screen
        .lines()
        .filter(|line| !line.trim().is_empty() && !seen.contains(line))
        .collect();
    let picker = painted && picker_open(&screen);
    let (mut navigates, mut filters, mut closes) = (None, None, None);
    if picker {
        let multi_row = picker_rows(&screen) > ONE_ROW;
        // A two-pane picker opens on the brands column: step right so ↓ walks the items.
        if pane_geometry(&screen).joined {
            tmux.key("Right")?;
            settle()?;
        }
        let cursor_before = cursor_line(&tmux.capture()?);
        tmux.key("Down")?;
        let moved = settle()?;
        navigates = outcome(
            multi_row,
            !cursor_before.is_empty() && cursor_line(&moved) != cursor_before,
        );
        let rows_before = picker_rows(&moved);
        tmux.type_text(UNMATCHABLE_QUERY)?;
        let filtered = settle()?;
        filters = outcome(multi_row, picker_rows(&filtered) < rows_before);
        tmux.key("C-u")?;
        settle()?;
        tmux.key("Escape")?;
        closes = outcome(true, !picker_open(&settle()?));
    }
    Ok(Probe {
        command: command.to_string(),
        painted,
        paint_ms,
        frames: box_frames(&tmux.history()?).saturating_sub(frames_before),
        fits: painted && frame_fits(&screen),
        picker,
        navigates,
        filters,
        closes,
        new_paint: new_paint.join("\n"),
    })
}
