//! What `subscriptions list --json` must answer.
//!
//! One row per joined subscription, in a deterministic order, each
//! carrying exactly the documented fields. A block still in force is
//! reported as the refusal in the way; a block that has lapsed is not
//! reported at all, and no vault payload appears anywhere.

use super::*;

/// The fields a row carries — exactly these, no more.
const ROW_FIELDS: [&str; 5] = [
    "expires_at",
    "last_redeem_error",
    "provider",
    "state",
    "subscription_id",
];

/// How many of the fixture's credentials are live, and the headline
/// the lines report must lead with.
pub(crate) const LIVE_COUNT: usize = 2;
pub(crate) const HEADLINE: &str = "2 of 6 subscription credentials are live";

/// The rows the fixture must produce, in the order the report must
/// place them: subscription id, provider, state, expiry instant, and
/// the refusal in the way.
fn expected(facts: &LedgerFacts) -> [(&'static str, &'static str, &'static str, Option<i64>, Option<&str>); 6] {
    [
        (
            "brama-sub-fixture-claude-code-primary",
            "claude-code",
            "unknown",
            None,
            None,
        ),
        (
            "brama-sub-fixture-codex-primary",
            "codex",
            "burnt",
            Some(facts.burnt),
            Some(facts.burnt_cause),
        ),
        (
            "brama-sub-fixture-codex-retired",
            "codex",
            "burnt",
            None,
            Some("retired by an operator"),
        ),
        (
            "brama-sub-fixture-codex-secondary",
            "codex",
            "expired",
            Some(facts.expired),
            Some(facts.active_reason),
        ),
        (
            "brama-sub-fixture-kimi-primary",
            "kimi",
            "live",
            Some(facts.live),
            None,
        ),
        (
            "brama-sub-fixture-kimi-secondary",
            "kimi",
            "live",
            Some(facts.short),
            None,
        ),
    ]
}

/// Check the whole report and answer with its rows, which the trace
/// records.
pub(crate) fn assert_report<'a>(
    report: &'a Value,
    facts: &LedgerFacts,
) -> Result<&'a Vec<Value>, String> {
    let providers = report["providers"]
        .as_array()
        .ok_or("`providers` is an array")?;
    if report.as_object().map(|o| o.keys().collect::<Vec<_>>())
        != Some(vec![&"providers".to_string()])
    {
        return Err("the pool report carries exactly one key".into());
    }

    let expected = expected(facts);
    if providers.len() != expected.len() {
        return Err(
            "the pool lists exactly the joined subscriptions, deterministically ordered".into(),
        );
    }

    let mut ids = BTreeSet::new();
    for (i, (row, exp)) in providers.iter().zip(expected).enumerate() {
        let (id, provider, state_word, expiry, error) = exp;
        if row["subscription_id"] != id
            || row["provider"] != provider
            || row["state"] != state_word
        {
            return Err(format!("row {i} ({id}) state/provider/id mismatch: {row}"));
        }
        if !ids.insert(id) {
            return Err(
                "a subscription known to both the listing and the ledger is one row, not two"
                    .into(),
            );
        }
        assert_row_fields(row, i, id)?;
        if row["last_redeem_error"] != error.map(Value::from).unwrap_or(Value::Null) {
            return Err(format!("row {i} ({id}) last_redeem_error"));
        }
        assert_expiry(row, i, id, expiry)?;
    }

    if report.to_string().contains(VAULT_PAYLOAD) {
        return Err("no vault item payload reaches the pool report".into());
    }
    Ok(providers)
}

fn assert_row_fields(row: &Value, index: usize, id: &str) -> Result<(), String> {
    let keys = row
        .as_object()
        .ok_or_else(|| format!("row {index} ({id}) is an object"))?
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if keys != ROW_FIELDS.iter().map(|s| s.to_string()).collect() {
        return Err(format!(
            "row {index} ({id}) carries exactly the documented fields"
        ));
    }
    Ok(())
}

/// The expiry is the provider's own instant, written so a human can
/// read it — or null when the ledger states none.
fn assert_expiry(row: &Value, index: usize, id: &str, expiry: Option<i64>) -> Result<(), String> {
    let Some(ms) = expiry else {
        if !row["expires_at"].is_null() {
            return Err(format!("row {index} ({id}) states no expiry"));
        }
        return Ok(());
    };
    let text = row["expires_at"]
        .as_str()
        .ok_or_else(|| format!("row {index} ({id}) states an expiry"))?;
    let parsed = chrono::DateTime::parse_from_rfc3339(text)
        .map_err(|_| format!("row {index} ({id}) expiry is an instant a human reads"))?;
    if parsed.timestamp_millis() != ms {
        return Err(format!(
            "row {index} ({id}) expiry is the provider's own instant"
        ));
    }
    Ok(())
}

/// The lines report leads with the live count, names the burnt
/// subscription and its cause, and carries neither the lapsed block
/// nor any vault payload.
pub(crate) fn assert_lines_report(output: &str, facts: &LedgerFacts) -> Result<(), String> {
    for needle in [HEADLINE, "brama-sub-fixture-codex-primary", facts.burnt_cause] {
        if !output.contains(needle) {
            return Err(format!(
                "the lines report leads with the live count: {HEADLINE}"
            ));
        }
    }
    if output.contains(facts.lapsed_reason) || output.contains(VAULT_PAYLOAD) {
        return Err("the lines report carries forbidden lapsed block or vault payload".into());
    }
    Ok(())
}
