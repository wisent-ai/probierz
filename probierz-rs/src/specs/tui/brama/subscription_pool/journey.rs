//! The run: read the pool twice, read it as lines, then watch the CLI
//! refuse a refresh without a reason, refuse a cost flag it does not
//! have, and refuse billable inference without an acknowledgement.

use super::*;

/// Exit status a clap argument error carries.
const USAGE_ERROR: i32 = 2;

/// Exit status the billable refusal carries: the command ran and
/// declined, rather than being misused.
const REFUSED_STATUS: i32 = 1;

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = context.required("TUI_CMD", "provide the released Brama executable")?;
    let source = manifest_source(context)?;
    let (revision, dirty) = source_identity(&source)?;

    let fixture = Fixture::build()?;
    let result = observe(&binary, &fixture, context, &source, &revision, dirty);
    fixture.remove();
    result
}

/// The source repository the Brama manifest points at.
fn manifest_source(context: &specs::Context) -> Result<String, String> {
    let manifest = fs::read_to_string(context.harness.join("apps/brama/probierz.yaml"))
        .map_err(|e| e.to_string())?;
    manifest
        .lines()
        .find_map(|l| l.strip_prefix("  - root: "))
        .map(str::trim)
        .map(str::to_string)
        .ok_or_else(|| "the Brama manifest must provide the source repository root".to_string())
}

/// The exact revision the executable was built from, and whether that
/// checkout carries uncommitted work.
fn source_identity(source: &str) -> Result<(String, bool), String> {
    let rev = common::run(
        "/usr/bin/git",
        &common::strings(&["-C", source, "rev-parse", "HEAD"]),
        None,
        &BTreeMap::new(),
        &[],
        None,
        COMMAND_TIMEOUT,
    )?;
    if !rev.status.success() {
        return Err(format!(
            "cannot resolve the Brama source revision: {}",
            rev.stderr
        ));
    }
    let revision = rev.stdout.trim().to_string();
    if !regex::Regex::new(r"^[0-9a-f]{40}$")
        .unwrap()
        .is_match(&revision)
    {
        return Err("the Brama source revision is not a full Git SHA".into());
    }
    let status = common::run(
        "/usr/bin/git",
        &common::strings(&["-C", source, "status", "--porcelain"]),
        None,
        &BTreeMap::new(),
        &[],
        None,
        COMMAND_TIMEOUT,
    )?;
    if !status.status.success() {
        return Err(format!(
            "cannot inspect the Brama source state: {}",
            status.stderr
        ));
    }
    Ok((revision, !status.stdout.trim().is_empty()))
}

fn observe(
    binary: &str,
    fixture: &Fixture,
    context: &specs::Context,
    source: &str,
    revision: &str,
    dirty: bool,
) -> Result<(), String> {
    let before = fixture.tree_fingerprint()?;
    let first = invoke(
        binary,
        &["subscriptions", "list", "--json"],
        &fixture.environment,
    )?;
    let exit_status = first.code();
    if exit_status != Some(0) {
        return Err(format!("subscriptions list --json exited {exit_status:?}"));
    }
    let report = parsed(&first, "subscriptions list --json")?;
    let providers = assert_report(&report, &fixture.facts)?;

    if !fs::read_to_string(&fixture.ledger_path)
        .map_err(|e| e.to_string())?
        .contains(fixture.facts.lapsed_reason)
    {
        return Err("the fixture ledger does hold the lapsed block that was not reported".into());
    }
    if fixture
        .router_invocations()
        .lines()
        .any(|line| line != "list")
    {
        return Err("the report used only the listing verb".into());
    }
    if fixture.tree_fingerprint()? != before {
        return Err("subscriptions list wrote nothing".into());
    }

    let again = invoke(
        binary,
        &["subscriptions", "list", "--json"],
        &fixture.environment,
    )?;
    if parsed(&again, "the second subscriptions list --json")? != report {
        return Err("two reads of an unchanged pool agree".into());
    }

    let lines = invoke(binary, &["subscriptions", "list"], &fixture.environment)?;
    assert_lines_report(&lines.combined(), &fixture.facts)?;
    assert_refusals(binary, fixture)?;

    write_trace(context, source, revision, dirty, exit_status, providers)
}

/// Three refusals, and no credential read behind any of them.
fn assert_refusals(binary: &str, fixture: &Fixture) -> Result<(), String> {
    let before = fixture.router_invocations();

    let no_reason = invoke(
        binary,
        &["subscription", "refresh", "codex", "--json"],
        &fixture.environment,
    )?;
    let no_output = no_reason.combined();
    if no_reason.code() != Some(USAGE_ERROR)
        || !no_output.contains("required arguments were not provided")
        || !no_output.contains("--reason <REASON>")
    {
        return Err("a refresh without a reason is refused and names the missing reason".into());
    }

    let cost = invoke(
        binary,
        &[
            "subscription",
            "refresh",
            "codex",
            "--reason",
            "probierz fixture: never sent",
            "--allow-provider-cost",
        ],
        &fixture.environment,
    )?;
    if cost.code() != Some(USAGE_ERROR)
        || !cost
            .combined()
            .contains("unexpected argument '--allow-provider-cost' found")
    {
        return Err("refresh has no billable-cost path to acknowledge".into());
    }

    let billable = invoke(
        binary,
        &[
            "test",
            "--agent-id",
            "wisent-app",
            "--model",
            "openai/default",
        ],
        &fixture.environment,
    )?;
    if billable.code() != Some(REFUSED_STATUS)
        || !billable
            .combined()
            .contains("refusing billable inference without explicit --allow-provider-cost")
    {
        return Err("the billable refusal names the missing cost acknowledgement".into());
    }
    if fixture.router_invocations() != before {
        return Err("the refused billable request read no credential".into());
    }
    Ok(())
}

/// Schema version of the trace every Probierz spec writes.
const TRACE_SCHEMA_VERSION: u64 = 1;

/// What this run observed, and the contracts it stands for.
fn write_trace(
    context: &specs::Context,
    source: &str,
    revision: &str,
    dirty: bool,
    exit_status: Option<i32>,
    providers: &Vec<Value>,
) -> Result<(), String> {
    common::write_trace(
        context,
        "brama-subscription-pool.trace.json",
        json!({
            "schemaVersion": TRACE_SCHEMA_VERSION,
            "kind": "probierz-brama-subscription-pool-trace",
            "evidenceLevel": "E2",
            "status": "completed",
            "source": {"root": source, "revision": revision, "dirty": dirty},
            "observation": {
                "list": {
                    "exitStatus": exit_status,
                    "providerCount": providers.len(),
                    "liveCount": LIVE_COUNT,
                    "headline": HEADLINE,
                    "rows": providers
                },
                "fixtureStateUnchanged": true,
                "vaultPayloadLeaked": false
            },
            "contracts": [
                "subscriptions list --json exits 0 and joins the deployment listing to the usage ledger as one row per subscription",
                "each row carries exactly provider, subscription_id, state, expires_at and last_redeem_error",
                "state is one of live, expired, burnt, unknown; expires_at is the provider instant or null",
                "a block still in force is reported as the refusal in the way; a lapsed block is not",
                "the report prints no vault payload and no credential-shaped field",
                "reading the pool writes nothing, calls only the listing verb, and answers the same twice",
                "the lines report leads with how many credentials are live",
                "subscription refresh without --reason is refused, names the missing reason, and contacts nothing",
                "refresh has no cost-acknowledgement flag; the billable path refuses without --allow-provider-cost"
            ]
        }),
    )
}
