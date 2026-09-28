//! Measurements taken from a captured pane: how many boxed frames it holds,
//! and whether a two-pane view is really two panes. A pane is a pane when
//! the divider is part of the frame (`┬`…`┴`) and sits at one column on every
//! row; "a dot and a bar somewhere" is what a flat list fakes by accident.

use std::collections::BTreeMap;

/// Boxed frames (`╭` top borders) in `capture`.
pub(crate) fn box_frames(capture: &str) -> usize {
    capture.matches('╭').count()
}

#[derive(Default)]
pub(crate) struct PaneGeometry {
    /// The `┬`/`┴` joints that make the divider part of the border.
    pub(crate) joined: bool,
    /// Rows whose divider sits at the frame's `┬` column.
    pub(crate) aligned_rows: usize,
    /// Split rows whose brands column carries a state dot.
    pub(crate) dots: usize,
    /// Item rows whose figures end at the most common right edge.
    pub(crate) aligned_metrics: usize,
}

pub(crate) fn pane_geometry(capture: &str) -> PaneGeometry {
    let lines: Vec<Vec<char>> = capture.lines().map(|line| line.chars().collect()).collect();
    let top = lines.iter().find(|line| line.contains(&'┬'));
    let bottom = lines.iter().any(|line| line.contains(&'┴'));
    let Some(column) = top.and_then(|line| line.iter().position(|c| *c == '┬')) else {
        return PaneGeometry::default();
    };
    let split: Vec<&Vec<char>> = lines
        .iter()
        .filter(|line| {
            let text: String = line.iter().collect();
            line.first() == Some(&'│')
                && text.trim_end().ends_with('│')
                && line.get(column) == Some(&'│')
        })
        .collect();
    let dots = split
        .iter()
        .filter(|line| line[..column].iter().any(|c| *c == '●' || *c == '○'))
        .count();
    let mut tally: BTreeMap<usize, usize> = BTreeMap::new();
    for line in &split {
        let right: String = line[column..].iter().collect();
        let trimmed = right.trim_end().trim_end_matches('│').trim_end();
        if trimmed.ends_with(['◫', '█', '│']) {
            *tally.entry(trimmed.chars().count()).or_default() += 1;
        }
    }
    PaneGeometry {
        joined: bottom,
        aligned_rows: split.len(),
        dots,
        aligned_metrics: tally.values().copied().max().unwrap_or_default(),
    }
}
