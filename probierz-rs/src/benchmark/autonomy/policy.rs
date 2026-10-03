//! The operator's standing decision, written once instead of given for each
//! product: whose GitHub account new products are created under, how many
//! the loop may create in a week, how much a pursuit of one lost case may
//! spend, and how many losses of one product a cycle hands to pursuits.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::failure::Failure;

const POINT: &str = "benchmark.autonomy";
/// Where the policy lives in a harness when no file is named.
pub(super) const POLICY_FILE: &str = "autonomy.yaml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Policy {
    pub schema_version: u32,
    /// The GitHub owner new products' repositories are created under.
    pub owner: String,
    /// How many products the loop may create in seven days.
    pub products_per_week: usize,
    /// How many of a topic's newest observations the scout reads.
    pub observations: usize,
    /// Drafts allowed per model question.
    pub rounds: u32,
    /// Cases in a new product's first suite.
    pub cases: usize,
    /// The most one pursuit of a lost case may spend, in US dollars.
    pub pursuit_budget_usd: String,
    /// How many lost cases of one suite a cycle hands to pursuits.
    pub pursuits_per_product: usize,
}

/// The policy, the file it was read from and the SHA-256 of its bytes, which
/// every adoption records as its authority.
pub(super) fn load(harness: &Path, explicit: Option<&Path>) -> Result<(PathBuf, Policy, String), Failure> {
    let file = explicit
        .map(Path::to_path_buf)
        .unwrap_or_else(|| harness.join(POLICY_FILE));
    let text = std::fs::read_to_string(&file).map_err(|error| {
        Failure::config(
            POINT,
            format!(
                "the loop acts only under a written policy (schemaVersion, owner, productsPerWeek, observations, rounds, cases, pursuitBudgetUsd, pursuitsPerProduct), and {} cannot be read: {error}",
                file.display()
            ),
        )
    })?;
    let policy: Policy = serde_yaml::from_str(&text)
        .map_err(|error| Failure::config(POINT, format!("{} is not a policy: {error}", file.display())))?;
    if policy.schema_version != 1 {
        return Err(Failure::config(POINT, format!("{}: schemaVersion must be 1", file.display())));
    }
    if policy.observations == 0 || policy.rounds == 0 || policy.cases == 0 {
        return Err(Failure::config(
            POINT,
            format!("{}: observations, rounds and cases must be at least 1", file.display()),
        ));
    }
    if !policy
        .pursuit_budget_usd
        .parse::<f64>()
        .is_ok_and(|dollars| dollars.is_finite() && dollars > 0.0)
    {
        return Err(Failure::config(
            POINT,
            format!(
                "{}: pursuitBudgetUsd {:?} is not a positive amount of US dollars",
                file.display(),
                policy.pursuit_budget_usd
            ),
        ));
    }
    let digest = hex::encode(Sha256::digest(text.as_bytes()));
    Ok((file, policy, digest))
}

/// Products the loop created in the last seven days, from its ledger.
pub(super) fn adopted_this_week(ledger: &Path) -> usize {
    let since = chrono::Utc::now() - chrono::Duration::days(7);
    std::fs::read_to_string(ledger)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|entry| {
            entry["at"]
                .as_str()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .is_some_and(|at| at >= since)
        })
        .count()
}

/// Add one adoption to the ledger the weekly limit is counted from.
pub(super) fn append(ledger: &Path, entry: &serde_json::Value) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(ledger)?;
    writeln!(file, "{entry}")
}
