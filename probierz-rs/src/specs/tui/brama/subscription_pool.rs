use crate::specs::{self, tui::common};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
fn fingerprint(root: &Path) -> Result<String, String> {
    fn walk(root: &Path, rows: &mut Vec<String>) -> Result<(), String> {
        if !root.exists() {
            return Ok(());
        }
        let mut entries = fs::read_dir(root)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if p.is_dir() {
                rows.push(format!("{}/", p.display()));
                walk(&p, rows)?;
            } else {
                let bytes = fs::read(&p).map_err(|e| e.to_string())?;
                let modified = e
                    .metadata()
                    .map_err(|e| e.to_string())?
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                rows.push(format!(
                    "{}\t{}\t{}\t{}",
                    p.display(),
                    bytes.len(),
                    hex::encode(Sha256::digest(&bytes)),
                    modified
                ));
            }
        }
        Ok(())
    }
    let mut rows = Vec::new();
    walk(root, &mut rows)?;
    Ok(rows.join("\n"))
}
fn invoke(
    binary: &str,
    args: &[&str],
    env: &BTreeMap<String, String>,
) -> Result<common::Output, String> {
    common::run(
        binary,
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        None,
        env,
        &[],
        None,
        Duration::from_secs(30),
    )
}
fn parsed(out: &common::Output, label: &str) -> Result<Value, String> {
    let combined = out.combined();
    let start = combined
        .find('{')
        .ok_or_else(|| format!("{label} emitted no JSON"))?;
    serde_json::from_str(combined[start..].trim())
        .map_err(|e| format!("{label} emitted a non-structured JSON value: {e}"))
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = context.required("TUI_CMD", "provide the released Brama executable")?;
    let manifest = fs::read_to_string(context.harness.join("apps/brama/probierz.yaml"))
        .map_err(|e| e.to_string())?;
    let source = manifest
        .lines()
        .find_map(|l| l.strip_prefix("  - root: "))
        .map(str::trim)
        .ok_or("the Brama manifest must provide the source repository root")?;
    let rev = common::run(
        "/usr/bin/git",
        &common::strings(&["-C", source, "rev-parse", "HEAD"]),
        None,
        &BTreeMap::new(),
        &[],
        None,
        Duration::from_secs(30),
    )?;
    if !rev.status.success() {
        return Err(format!(
            "cannot resolve the Brama source revision: {}",
            rev.stderr
        ));
    }
    let revision = rev.stdout.trim();
    if !regex::Regex::new(r"^[0-9a-f]{40}$")
        .unwrap()
        .is_match(revision)
    {
        return Err("the Brama source revision is not a full Git SHA".into());
    }
    let dirty = common::run(
        "/usr/bin/git",
        &common::strings(&["-C", source, "status", "--porcelain"]),
        None,
        &BTreeMap::new(),
        &[],
        None,
        Duration::from_secs(30),
    )?;
    if !dirty.status.success() {
        return Err(format!(
            "cannot inspect the Brama source state: {}",
            dirty.stderr
        ));
    }
    let temp = common::scratch("brama-subscription-pool")?;
    let state = temp.join("state");
    let home = temp.join("home");
    let bin = temp.join("bin");
    fs::create_dir_all(&state).map_err(|e| e.to_string())?;
    fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let burnt = now - 1_800_000;
    let expired = now - 600_000;
    let live = now + 86_400_000;
    let short = now + 3_600_000;
    let payload = "probierz-fixture-vault-payload-must-never-be-printed";
    let item = |id: &str, provider: &str, sub: &str, agent: &str, deleted: bool| json!({"id":id,"type":"subscription","tags":["brama:subscription",format!("brama:agent:{agent}"),format!("brama:provider:{provider}"),format!("brama:id:{sub}")],"deleted":deleted,"updated_at":"2026-08-01T00:00:00Z","versions":[{"version":1,"value":payload}]});
    let vault = json!([
        item(
            "provider:codex:primary",
            "codex",
            "brama-sub-fixture-codex-primary",
            "wisent-app",
            false
        ),
        item(
            "provider:codex:secondary",
            "codex",
            "brama-sub-fixture-codex-secondary",
            "wisent-app",
            false
        ),
        item(
            "provider:kimi:primary",
            "kimi",
            "brama-sub-fixture-kimi-primary",
            "other-agent",
            false
        ),
        item(
            "provider:kimi:secondary",
            "kimi",
            "brama-sub-fixture-kimi-secondary",
            "wisent-app",
            false
        ),
        item(
            "provider:claude_code:primary",
            "claude_code",
            "brama-sub-fixture-claude-code-primary",
            "wisent-app",
            false
        ),
        item(
            "provider:codex:removed",
            "codex",
            "brama-sub-fixture-codex-removed",
            "wisent-app",
            true
        )
    ]);
    let burnt_cause = "invalid_grant: refresh token is no longer accepted";
    let active_reason = "429 from provider: this account is over its plan";
    let lapsed_reason = "429 from provider: a lapsed block that must not be reported";
    let ledger = json!({"subscriptions":{"brama-sub-fixture-codex-primary":{"provider":"codex","credential":{"state":"needs_reauthorization","cause":burnt_cause,"recorded_at_ms":now-3_600_000,"expires_at_ms":burnt}},"brama-sub-fixture-codex-secondary":{"provider":"codex","credential":{"state":"active","recorded_at_ms":now-7_200_000,"expires_at_ms":expired},"block":{"blocked_until_ms":now+1_800_000,"reason":active_reason,"recorded_at_ms":now-900_000}},"brama-sub-fixture-kimi-primary":{"provider":"kimi","credential":{"state":"active","recorded_at_ms":now-300_000,"expires_at_ms":live}},"brama-sub-fixture-kimi-secondary":{"provider":"kimi","credential":{"state":"active","recorded_at_ms":now-300_000,"expires_at_ms":short},"block":{"blocked_until_ms":now-60_000,"reason":lapsed_reason,"recorded_at_ms":now-3_600_000}},"brama-sub-fixture-codex-retired":{"provider":"codex","credential":{"state":"disabled","cause":"retired by an operator","recorded_at_ms":now-86_400_000}}}});
    let vault_path = temp.join("vault-list.json");
    let ledger_path = state.join("subscription-usage.json");
    common::write_json(&vault_path, &vault)?;
    common::write_json(&ledger_path, &ledger)?;
    fs::write(state.join("journal.jsonl"), "").map_err(|e| e.to_string())?;
    let router = bin.join("entitlements-router");
    let router_log = temp.join("entitlements-router.invocations");
    fs::write(&router,format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = list ]; then cat '{}'; exit 0; fi\nprintf 'fixture entitlements router refuses %s: a Probierz fixture redeems no capability\\n' \"$1\" >&2\nexit 3\n",router_log.display(),vault_path.display())).map_err(|e|e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&router, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let env = common::env_map([
        ("HOME", home.to_string_lossy().as_ref()),
        (
            "XDG_STATE_HOME",
            temp.join("xdg-state").to_string_lossy().as_ref(),
        ),
        ("BRAMA_STATE_DIR", state.to_string_lossy().as_ref()),
        (
            "BRAMA_SUBSCRIPTION_USAGE_FILE",
            ledger_path.to_string_lossy().as_ref(),
        ),
        (
            "BRAMA_MODEL_CATALOG_CACHE",
            temp.join("model-catalog.json").to_string_lossy().as_ref(),
        ),
        (
            "BRAMA_PERF_PATH",
            temp.join("perf.json").to_string_lossy().as_ref(),
        ),
        (
            "BRAMA_DONATED_SUBSCRIPTIONS_FILE",
            temp.join("donated-subscriptions.json")
                .to_string_lossy()
                .as_ref(),
        ),
        ("BRAMA_SUBSCRIPTION_CATALOG", "{\"items\":[]}"),
        (
            "SKARBIEC_CAPABILITY_ROUTES_FILE",
            temp.join("capability-routes.json")
                .to_string_lossy()
                .as_ref(),
        ),
        ("ENTITLEMENTS_ROUTER_BIN", router.to_string_lossy().as_ref()),
    ]);
    let result = (|| {
        let before = format!("{}\n--\n{}", fingerprint(&state)?, fingerprint(&home)?);
        let first = invoke(&binary, &["subscriptions", "list", "--json"], &env)?;
        if first.code() != Some(0) {
            return Err(format!(
                "subscriptions list --json exited {:?}",
                first.code()
            ));
        }
        let report = parsed(&first, "subscriptions list --json")?;
        let providers = report["providers"]
            .as_array()
            .ok_or("`providers` is an array")?;
        if report.as_object().map(|o| o.keys().collect::<Vec<_>>())
            != Some(vec![&"providers".to_string()])
        {
            return Err("the pool report carries exactly one key".into());
        }
        let expected = [
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
                Some(burnt),
                Some(burnt_cause),
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
                Some(expired),
                Some(active_reason),
            ),
            (
                "brama-sub-fixture-kimi-primary",
                "kimi",
                "live",
                Some(live),
                None,
            ),
            (
                "brama-sub-fixture-kimi-secondary",
                "kimi",
                "live",
                Some(short),
                None,
            ),
        ];
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
            let keys = row
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>();
            if keys
                != [
                    "expires_at",
                    "last_redeem_error",
                    "provider",
                    "state",
                    "subscription_id",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect()
            {
                return Err(format!(
                    "row {i} ({id}) carries exactly the documented fields"
                ));
            }
            if row["last_redeem_error"] != error.map(Value::from).unwrap_or(Value::Null) {
                return Err(format!("row {i} ({id}) last_redeem_error"));
            }
            if let Some(ms) = expiry {
                let text = row["expires_at"]
                    .as_str()
                    .ok_or_else(|| format!("row {i} ({id}) states an expiry"))?;
                let parsed = chrono::DateTime::parse_from_rfc3339(text)
                    .map_err(|_| format!("row {i} ({id}) expiry is an instant a human reads"))?;
                if parsed.timestamp_millis() != ms {
                    return Err(format!(
                        "row {i} ({id}) expiry is the provider's own instant"
                    ));
                }
            } else if !row["expires_at"].is_null() {
                return Err(format!("row {i} ({id}) states no expiry"));
            }
        }
        let text = report.to_string();
        if text.contains(payload) {
            return Err("no vault item payload reaches the pool report".into());
        }
        if !fs::read_to_string(&ledger_path)
            .map_err(|e| e.to_string())?
            .contains(lapsed_reason)
        {
            return Err(
                "the fixture ledger does hold the lapsed block that was not reported".into(),
            );
        }
        let invocations = fs::read_to_string(&router_log).unwrap_or_default();
        if invocations.lines().any(|l| l != "list") {
            return Err("the report used only the listing verb".into());
        }
        if format!("{}\n--\n{}", fingerprint(&state)?, fingerprint(&home)?) != before {
            return Err("subscriptions list wrote nothing".into());
        }
        let again = invoke(&binary, &["subscriptions", "list", "--json"], &env)?;
        if parsed(&again, "the second subscriptions list --json")? != report {
            return Err("two reads of an unchanged pool agree".into());
        }
        let lines = invoke(&binary, &["subscriptions", "list"], &env)?;
        let line_output = lines.combined();
        let headline = "2 of 6 subscription credentials are live";
        for needle in [headline, "brama-sub-fixture-codex-primary", burnt_cause] {
            if !line_output.contains(needle) {
                return Err(format!(
                    "the lines report leads with the live count: {headline}"
                ));
            }
        }
        if line_output.contains(lapsed_reason) || line_output.contains(payload) {
            return Err("the lines report carries forbidden lapsed block or vault payload".into());
        }
        let before_inv = fs::read_to_string(&router_log).unwrap_or_default();
        let no_reason = invoke(
            &binary,
            &["subscription", "refresh", "codex", "--json"],
            &env,
        )?;
        let no_output = no_reason.combined();
        if no_reason.code() != Some(2)
            || !no_output.contains("required arguments were not provided")
            || !no_output.contains("--reason <REASON>")
        {
            return Err(
                "a refresh without a reason is refused and names the missing reason".into(),
            );
        }
        let cost = invoke(
            &binary,
            &[
                "subscription",
                "refresh",
                "codex",
                "--reason",
                "probierz fixture: never sent",
                "--allow-provider-cost",
            ],
            &env,
        )?;
        if cost.code() != Some(2)
            || !cost
                .combined()
                .contains("unexpected argument '--allow-provider-cost' found")
        {
            return Err("refresh has no billable-cost path to acknowledge".into());
        }
        let billable = invoke(
            &binary,
            &[
                "test",
                "--agent-id",
                "wisent-app",
                "--model",
                "openai/default",
            ],
            &env,
        )?;
        if billable.code() != Some(1)
            || !billable
                .combined()
                .contains("refusing billable inference without explicit --allow-provider-cost")
        {
            return Err("the billable refusal names the missing cost acknowledgement".into());
        }
        if fs::read_to_string(&router_log).unwrap_or_default() != before_inv {
            return Err("the refused billable request read no credential".into());
        }
        common::write_trace(
            context,
            "brama-subscription-pool.trace.json",
            json!({"schemaVersion":1,"kind":"probierz-brama-subscription-pool-trace","evidenceLevel":"E2","status":"completed","source":{"root":source,"revision":revision,"dirty":!dirty.stdout.trim().is_empty()},"observation":{"list":{"exitStatus":0,"providerCount":providers.len(),"liveCount":2,"headline":headline,"rows":providers},"fixtureStateUnchanged":true,"vaultPayloadLeaked":false},"contracts":["subscriptions list --json exits 0 and joins the deployment listing to the usage ledger as one row per subscription","each row carries exactly provider, subscription_id, state, expires_at and last_redeem_error","state is one of live, expired, burnt, unknown; expires_at is the provider instant or null","a block still in force is reported as the refusal in the way; a lapsed block is not","the report prints no vault payload and no credential-shaped field","reading the pool writes nothing, calls only the listing verb, and answers the same twice","the lines report leads with how many credentials are live","subscription refresh without --reason is refused, names the missing reason, and contacts nothing","refresh has no cost-acknowledgement flag; the billable path refuses without --allow-provider-cost"]}),
        )
    })();
    common::remove(&temp);
    result
}
