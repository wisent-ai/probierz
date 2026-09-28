//! jeden and omp walk the same script at one geometry in tmux, and the run
//! leaves an HTML report a person can flip through. jeden's own screens (not
//! the live catalogue) are also compared with text goldens, volatile values
//! masked. A missing golden is a failure; PROBIERZ_UPDATE_GOLDEN=1 writes
//! them, and the written files are reviewed and committed under tests/tui/golden.

use std::fs;
use std::path::Path;
use std::time::Duration;

use regex::Regex;

use crate::specs::tui::jeden::screens::tmux::Tmux;
use crate::specs::{self, tui::common};

/// A step gets a minute to paint.
const STEP_SECONDS: u64 = 60;

struct Step {
    label: &'static str,
    command: Option<&'static str>,
    shown: fn(&str) -> bool,
    golden: bool,
}

const JEDEN_STEPS: [Step; 3] = [
    Step {
        label: "welcome",
        command: None,
        shown: |pane| pane.contains("Tips") || pane.contains("Welcome back"),
        golden: true,
    },
    Step {
        label: "model-view",
        command: Some("/model --all"),
        shown: |pane| pane.contains("Select model route"),
        golden: false,
    },
    Step {
        label: "settings-view",
        command: Some("/settings"),
        shown: |pane| pane.contains("Jeden settings"),
        golden: true,
    },
];

const OMP_STEPS: [Step; 3] = [
    Step {
        label: "welcome",
        command: None,
        shown: |pane| pane.contains("Tips") || pane.contains("sessions"),
        golden: false,
    },
    Step {
        label: "model-view",
        command: Some("/models"),
        shown: |pane| pane.contains("All available models") || pane.contains("Roles"),
        golden: false,
    },
    Step {
        label: "settings-view",
        command: Some("/settings"),
        shown: |pane| pane.contains("Appearance"),
        golden: false,
    },
];

/// Volatile values masked before a golden comparison: build hashes, measured
/// speed, catalogue counts, costs, quota percents and dates.
fn normalized(text: &str) -> Result<String, String> {
    let masks = [
        (r"dev\.\d+\.[0-9a-f]+(?:\.dirty)?", "dev.X.HASH"),
        (r"\d+\.\d+s \d+t/s", "Ns Nt/s"),
        (r"\b\d{4,} models\b", "N models"),
        (r"\$\d+\.\d+", "$$X"),
        (r"\b\d{1,3}%", "Q%"),
        (r"\d{4}-\d{2}-\d{2}T[\d:.]+Z?", "DATE"),
    ];
    let mut out = text.to_string();
    for (pattern, mask) in masks {
        out = Regex::new(pattern)
            .map_err(|error| error.to_string())?
            .replace_all(&out, mask)
            .into_owned();
    }
    Ok(out)
}

fn walk(tmux: &Tmux, steps: &[Step]) -> Vec<(&'static str, Result<String, String>)> {
    let mut captures = Vec::new();
    for step in steps {
        let seen = step
            .command
            .map_or(Ok(()), |command| {
                tmux.key("Escape").and_then(|()| tmux.submit(command))
            })
            .and_then(|()| tmux.until(step.label, Duration::from_secs(STEP_SECONDS), step.shown))
            .map(|(pane, _)| pane);
        let failed = seen.is_err();
        captures.push((step.label, seen));
        if failed {
            break;
        }
    }
    captures
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Compare with the reviewed golden text; write it only when asked to.
fn golden(context: &specs::Context, label: &str, pane: &str) -> Result<(), String> {
    let path = context
        .harness
        .join(format!("tests/tui/golden/jeden-{label}.txt"));
    let text = normalized(pane)?;
    if context.optional("PROBIERZ_UPDATE_GOLDEN").is_some() {
        fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
            .map_err(|error| error.to_string())?;
        return fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()));
    }
    if !path.exists() {
        return Err(format!(
            "{label} has no reviewed golden at {}: run once with PROBIERZ_UPDATE_GOLDEN=1, review the written text and commit it",
            path.display()
        ));
    }
    let expected =
        fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    if expected == text {
        Ok(())
    } else {
        Err(format!("{label} differs from {}", path.display()))
    }
}

/// jeden-side-by-side: the walkthrough report, and jeden's screens against their goldens.
pub fn side_by_side(context: &specs::Context) -> Result<(), String> {
    crate::specs::tui::jeden::cli::network::brama(context)?;
    let omp_binary = context
        .optional("OMP_BIN")
        .unwrap_or_else(|| "omp".to_string());
    let binary = context
        .optional("TUI_CMD")
        .unwrap_or_else(|| "jeden".to_string());
    let home = crate::specs::tui::jeden::sandbox::home(true, true)?;
    let cwd = common::scratch("probierz-sidebyside")?;
    let jeden = Tmux::start(&format!("{binary} --cwd {}", cwd.display()), Some(&home))
        .map(|tmux| walk(&tmux, &JEDEN_STEPS));
    let omp = Tmux::start(&format!("{omp_binary} --allow-home"), None)
        .map(|tmux| walk(&tmux, &OMP_STEPS));
    common::remove(&home);
    common::remove(&cwd);
    let (jeden, omp) = (jeden?, omp?);

    let mut rows = String::new();
    let mut failures = Vec::new();
    for step in &JEDEN_STEPS {
        let pane = |walked: &[(&str, Result<String, String>)]| {
            walked
                .iter()
                .find(|(label, _)| *label == step.label)
                .map(|(_, seen)| match seen {
                    Ok(pane) => pane.clone(),
                    Err(error) => format!("(not seen: {error})"),
                })
        };
        let jeden_pane = pane(&jeden).unwrap_or_else(|| "(no capture)".into());
        let omp_pane = pane(&omp).unwrap_or_else(|| "(no capture)".into());
        rows.push_str(&format!(
            "<h2>{}</h2><div class=\"row\"><div><h3>jeden</h3><pre>{}</pre></div><div><h3>omp</h3><pre>{}</pre></div></div>",
            step.label,
            escape(&jeden_pane),
            escape(&omp_pane)
        ));
        if let Some((_, Ok(seen))) = jeden.iter().find(|(label, _)| *label == step.label) {
            if step.golden {
                if let Err(error) = golden(context, step.label, seen) {
                    failures.push(error);
                }
            }
        }
    }
    for (who, walked) in [("jeden", &jeden), ("omp", &omp)] {
        for (label, seen) in walked.iter() {
            if let Err(error) = seen {
                failures.push(format!("{who} step {label}: {error}"));
            }
        }
    }
    let report = context.artifacts.join("sidebyside.html");
    let html = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>jeden vs omp</title><style>body{{font:13px/1.35 Menlo,monospace;background:#0d1117;color:#e6edf3;padding:1em}}.row{{display:flex;gap:1em}}pre{{background:#161b22;border:1px solid #30363d;padding:1em;overflow:auto;max-width:49vw}}</style><h1>jeden vs omp</h1>{rows}"
    );
    fs::write(&report, html).map_err(|error| format!("{}: {error}", report.display()))?;
    context.media_typed("trace", report, "text/html");
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}
