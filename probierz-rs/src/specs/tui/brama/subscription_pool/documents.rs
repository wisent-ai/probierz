//! The two documents the product reads: the vault listing a stub
//! entitlements router serves, and the usage ledger beside it.
//!
//! Between them they cover every state the report distinguishes: a
//! credential the provider refused, one blocked right now, one whose
//! block has already lapsed, two that are live, one the ledger knows
//! and the listing does not, and one the listing marks deleted.

use super::*;

/// Vault items in this fixture have one version, their first.
const FIRST_ITEM_VERSION: u64 = 1;

/// Six joined subscriptions, one of them deleted, across three
/// providers and two agents.
pub(crate) fn vault_listing() -> Value {
    let item = |id: &str, provider: &str, sub: &str, agent: &str, deleted: bool| {
        json!({
            "id": id, "type": "subscription",
            "tags": [
                "brama:subscription",
                format!("brama:agent:{agent}"),
                format!("brama:provider:{provider}"),
                format!("brama:id:{sub}")
            ],
            "deleted": deleted,
            "updated_at": "2026-08-01T00:00:00Z",
            "versions": [{"version": FIRST_ITEM_VERSION, "value": VAULT_PAYLOAD}]
        })
    };
    json!([
        item("provider:codex:primary", "codex", "brama-sub-fixture-codex-primary", "wisent-app", false),
        item("provider:codex:secondary", "codex", "brama-sub-fixture-codex-secondary", "wisent-app", false),
        item("provider:kimi:primary", "kimi", "brama-sub-fixture-kimi-primary", "other-agent", false),
        item("provider:kimi:secondary", "kimi", "brama-sub-fixture-kimi-secondary", "wisent-app", false),
        item("provider:claude_code:primary", "claude_code", "brama-sub-fixture-claude-code-primary", "wisent-app", false),
        item("provider:codex:removed", "codex", "brama-sub-fixture-codex-removed", "wisent-app", true)
    ])
}

/// The instants and reasons the ledger records, which the report must
/// carry back verbatim.
pub(crate) struct LedgerFacts {
    pub(crate) now: i64,
    pub(crate) burnt: i64,
    pub(crate) expired: i64,
    pub(crate) live: i64,
    pub(crate) short: i64,
    pub(crate) burnt_cause: &'static str,
    pub(crate) active_reason: &'static str,
    pub(crate) lapsed_reason: &'static str,
}

/// The usage ledger: a burnt credential, one blocked right now, two
/// live ones — one of them carrying a block that has already lapsed —
/// and one the listing does not carry at all.
pub(crate) fn ledger(facts: &LedgerFacts) -> Value {
    let now = facts.now;
    json!({"subscriptions": {
        "brama-sub-fixture-codex-primary": {
            "provider": "codex",
            "credential": {"state": "needs_reauthorization", "cause": facts.burnt_cause,
                           "recorded_at_ms": now - RECORDED_HOUR_AGO_MS,
                           "expires_at_ms": facts.burnt}
        },
        "brama-sub-fixture-codex-secondary": {
            "provider": "codex",
            "credential": {"state": "active",
                           "recorded_at_ms": now - RECORDED_TWO_HOURS_AGO_MS,
                           "expires_at_ms": facts.expired},
            "block": {"blocked_until_ms": now + BLOCK_AHEAD_MS,
                      "reason": facts.active_reason,
                      "recorded_at_ms": now - BLOCK_RECORDED_AGO_MS}
        },
        "brama-sub-fixture-kimi-primary": {
            "provider": "kimi",
            "credential": {"state": "active",
                           "recorded_at_ms": now - RECORDED_RECENTLY_MS,
                           "expires_at_ms": facts.live}
        },
        "brama-sub-fixture-kimi-secondary": {
            "provider": "kimi",
            "credential": {"state": "active",
                           "recorded_at_ms": now - RECORDED_RECENTLY_MS,
                           "expires_at_ms": facts.short},
            "block": {"blocked_until_ms": now - BLOCK_LAPSED_AGO_MS,
                      "reason": facts.lapsed_reason,
                      "recorded_at_ms": now - RECORDED_HOUR_AGO_MS}
        },
        "brama-sub-fixture-codex-retired": {
            "provider": "codex",
            "credential": {"state": "disabled", "cause": "retired by an operator",
                           "recorded_at_ms": now - RECORDED_DAY_AGO_MS}
        }
    }})
}

/// How long ago each recorded state was written. None of them is read
/// by the report; they exist so the ledger looks like one a host would
/// actually have.
const RECORDED_RECENTLY_MS: i64 = 300_000;
const RECORDED_HOUR_AGO_MS: i64 = 3_600_000;
const RECORDED_TWO_HOURS_AGO_MS: i64 = 7_200_000;
const RECORDED_DAY_AGO_MS: i64 = 86_400_000;
const BLOCK_RECORDED_AGO_MS: i64 = 900_000;

/// The block still in force ends in half an hour; the lapsed one
/// ended a minute ago.
const BLOCK_AHEAD_MS: i64 = 1_800_000;
const BLOCK_LAPSED_AGO_MS: i64 = 60_000;
