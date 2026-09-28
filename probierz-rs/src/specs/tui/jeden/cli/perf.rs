//! jeden command latency: each command runs cold then warm, the samples go
//! into a JSON trace, and a run above its ceiling fails naming the command.

use std::time::Instant;

use serde_json::json;

use super::constants::{LOCAL_CEILING_MS, NETWORK_CEILING_MS};
use super::network::brama;
use super::succeeded;
use crate::specs::{self, tui::common};

/// (label, arguments, stdin) of one measured command.
type Command = (&'static str, &'static [&'static str], Option<&'static str>);

fn measure(
    context: &specs::Context,
    group: &str,
    commands: &[Command],
    ceiling: u128,
) -> Result<(), String> {
    let mut samples = Vec::new();
    let mut over = Vec::new();
    for (label, args, input) in commands {
        let mut runs = [0u128; 2];
        for run in runs.iter_mut() {
            let started = Instant::now();
            succeeded(context, args, *input)?;
            *run = started.elapsed().as_millis();
        }
        let [cold, warm] = runs;
        if cold >= ceiling || warm >= ceiling {
            over.push(format!("{label} (cold {cold} ms, warm {warm} ms)"));
        }
        samples.push(json!({ "command": label, "coldMs": cold as u64, "warmMs": warm as u64 }));
    }
    common::write_trace(
        context,
        &format!("jeden-perf-{group}.json"),
        json!({ "samples": samples }),
    )?;
    if over.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "above the {ceiling} ms ceiling: {}",
            over.join("; ")
        ))
    }
}

/// jeden-perf-local: local commands answer within the local ceiling, cold and warm.
pub fn local(context: &specs::Context) -> Result<(), String> {
    measure(
        context,
        "local",
        &[
            ("--version", &["--version"], None),
            ("config", &["config"], None),
            ("tools", &["tools"], None),
            ("completions bash", &["completions", "bash"], None),
            (
                "gallery --theme nord",
                &["gallery", "--theme", "nord"],
                None,
            ),
            ("/settings (picker export)", &[], Some("/settings\n")),
        ],
        LOCAL_CEILING_MS,
    )
}

/// jeden-perf-network: Brama-backed commands answer within the network ceiling, cold and warm.
pub fn network(context: &specs::Context) -> Result<(), String> {
    brama(context)?;
    measure(
        context,
        "network",
        &[
            ("/model (picker export)", &[], Some("/model\n")),
            ("/usage (quota view)", &[], Some("/usage\n")),
            ("stats --summary", &["stats", "--summary"], None),
            ("doctor", &["doctor"], None),
        ],
        NETWORK_CEILING_MS,
    )
}
