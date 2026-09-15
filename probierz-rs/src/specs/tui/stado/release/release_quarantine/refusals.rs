//! The clears that must be refused: without a digest, a target or a reason, with a
//! blank reason, and for a digest nobody quarantined; none may touch the state.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use crate::specs::tui::stado::fleet_fixture::{
    self as fixture, FleetFixture, FIXTURE_HOST, FIXTURE_PRODUCT,
};

pub(super) fn refused_clears(
    fleet: &mut FleetFixture,
    digest: &str,
    state_path: &Path,
    state_before: &str,
) -> Result<serde_json::Map<String, Value>, String> {
    let absent_digest = "f".repeat(64);
    let refusals = vec![
        (
            "no reason",
            vec![
                "release",
                "quarantine",
                "clear",
                FIXTURE_PRODUCT,
                "--target",
                FIXTURE_HOST,
                "--digest",
                digest,
            ],
            "--reason <REASON>",
        ),
        (
            "no digest",
            vec![
                "release",
                "quarantine",
                "clear",
                FIXTURE_PRODUCT,
                "--target",
                FIXTURE_HOST,
                "--reason",
                "because",
            ],
            "--digest <DIGEST>",
        ),
        (
            "no target",
            vec![
                "release",
                "quarantine",
                "clear",
                FIXTURE_PRODUCT,
                "--digest",
                digest,
                "--reason",
                "because",
            ],
            "--target <TARGET>",
        ),
        (
            "blank reason",
            vec![
                "release",
                "quarantine",
                "clear",
                FIXTURE_PRODUCT,
                "--target",
                FIXTURE_HOST,
                "--digest",
                digest,
                "--reason",
                "   ",
            ],
            "--reason must say why this digest is being retried",
        ),
        (
            "a digest nobody quarantined",
            vec![
                "release",
                "quarantine",
                "clear",
                FIXTURE_PRODUCT,
                "--target",
                FIXTURE_HOST,
                "--digest",
                &absent_digest,
                "--reason",
                "aiming at a digest nobody quarantined",
            ],
            "ffffffffffffffff",
        ),
    ];
    let mut refusal_statuses = serde_json::Map::new();
    for (name, args, expected) in refusals {
        let attempt = fleet.invoke(&args)?;
        fixture::ensure(
            attempt.status != 0,
            format!("clear with {name} must be refused"),
        )?;
        fixture::ensure(
            attempt.output.contains(expected),
            format!("the \"{name}\" refusal does not say why"),
        )?;
        fixture::ensure(
            fs::read_to_string(&state_path).map_err(|error| error.to_string())? == state_before,
            format!("a clear refused for {name} still rewrote the rollout state"),
        )?;
        refusal_statuses.insert(name.to_string(), json!(attempt.status));
    }
    let mut state_files = fs::read_dir(&fleet.state_dir)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    state_files.sort();
    fixture::ensure(
        state_files == vec![format!("{FIXTURE_PRODUCT}.json")],
        "a refused clear left a backup or an audit file behind",
    )?;
    Ok(refusal_statuses)
}
