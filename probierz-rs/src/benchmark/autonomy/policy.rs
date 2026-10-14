//! The operator's standing decision, written once instead of given for each
//! product: whose GitHub account new products are created under, how many
//! the loop may create in a week, how much a pursuit of one lost case may
//! spend, and how many losses of one product a cycle hands to pursuits.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::failure::Failure;

const POINT: &str = "benchmark.autonomy";
/// Where the policy lives in a harness when no file is named.
pub(super) const POLICY_FILE: &str = "autonomy.yaml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(in crate::benchmark::autonomy) struct Policy {
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
    /// Without Trends and Competitors: a directory of `<topic>.json` files,
    /// each holding what they would have answered for its topic (trend,
    /// observations, candidates); the cycle then scouts those topics and
    /// neither watches, ingests nor asks Trends for a list.
    #[serde(default)]
    pub evidence_dir: Option<PathBuf>,
}

/// The policy, the file it was read from and the SHA-256 of its bytes, which
/// every adoption records as its authority.
pub(super) fn load(
    harness: &Path,
    explicit: Option<&Path>,
) -> Result<(PathBuf, Policy, String), Failure> {
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
    let mut policy: Policy = serde_yaml::from_str(&text).map_err(|error| {
        Failure::config(
            POINT,
            format!("{} is not a policy: {error}", file.display()),
        )
    })?;
    if policy.schema_version != 1 {
        return Err(Failure::config(
            POINT,
            format!("{}: schemaVersion must be 1", file.display()),
        ));
    }
    if policy.observations == 0 || policy.rounds == 0 || policy.cases == 0 {
        return Err(Failure::config(
            POINT,
            format!(
                "{}: observations, rounds and cases must be at least 1",
                file.display()
            ),
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
    if let Some(directory) = &policy.evidence_dir {
        let resolved = if directory.is_absolute() {
            directory.clone()
        } else {
            harness.join(directory)
        };
        if !resolved.is_dir() {
            return Err(Failure::config(
                POINT,
                format!(
                    "{}: evidenceDir {} is not a directory of <topic>.json evidence files",
                    file.display(),
                    resolved.display()
                ),
            ));
        }
        policy.evidence_dir = Some(resolved);
    }
    let digest = crate::evidence::sha256_bytes(text.as_bytes());
    Ok((file, policy, digest))
}

/// Products the loop created in the last seven days, from its ledger; none
/// when there is no ledger yet. A ledger that cannot be read is refused:
/// counted as empty, the weekly limit would never be reached.
pub(super) fn adopted_this_week(ledger: &Path) -> Result<usize, Failure> {
    let since = chrono::Utc::now() - chrono::Duration::days(7);
    let text = match std::fs::read_to_string(ledger) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(Failure::config(
                POINT,
                format!(
                    "cannot read the adoption ledger {}, so the weekly limit cannot be counted: {error}",
                    ledger.display()
                ),
            ))
        }
    };
    // Every entry states when it was adopted; one that does not, or is not
    // JSON, is refused: left uncounted, the weekly limit could be exceeded.
    let mut adopted = Vec::new();
    for line in text.lines() {
        let at = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|entry| {
                entry["at"]
                    .as_str()
                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            })
            .ok_or_else(|| {
                Failure::config(
                    POINT,
                    format!(
                        "the adoption ledger {} holds an entry without an RFC 3339 time, so the weekly limit cannot be counted: {line}",
                        ledger.display()
                    ),
                )
            })?;
        adopted.push(at);
    }
    Ok(adopted.into_iter().filter(|at| *at >= since).count())
}

/// Add one adoption to the ledger the weekly limit is counted from.
pub(in crate::benchmark::autonomy) fn append(
    ledger: &Path,
    entry: &serde_json::Value,
) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ledger)?;
    writeln!(file, "{entry}")
}
