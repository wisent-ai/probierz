use std::fs;
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};

use crate::failure::iso_timestamp;
use crate::specs;

use super::stado_fleet_fixture::{self as fixture, FleetFixture, FIXTURE_HOST};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let source = fixture::source_identity()?;
    let mut fleet = FleetFixture::open(context, "stado-host-gates")?;
    let result = run_fixture(context, &source, &mut fleet);
    let close = fleet.close();
    result.and(close)
}

fn run_fixture(
    context: &specs::Context,
    source: &Value,
    fleet: &mut FleetFixture,
) -> Result<(), String> {
    let disk_policy = json!({"mode":"enforce","low_free_gb":10,"target_free_gb":20,"check_interval_seconds":300,"max_bytes_per_pass":21474836480u64,"max_items_per_pass":50,"max_scan_items":10000,"cleaners":{}});
    let claiming = json!({"disk_pressure_unresolved":false,"disk_cleanup_policy_known":true,"queue_paused":false,"pinned_only":false});
    fleet.registry(&fixture::fixture_registry(
        json!({"disk_cleanup":disk_policy}),
        None,
    )?)?;
    fleet.publish_capacity(claiming.clone(), 2, true, None)?;
    let healthy = gates(fleet)?;
    fixture::ensure(
        healthy.status == 0,
        format!("a claiming host must exit zero: {}", healthy.output),
    )?;
    fixture::ensure(
        healthy.json["host"] == FIXTURE_HOST && healthy.json["claiming"] == true,
        format!("healthy host report is wrong: {}", healthy.json),
    )?;
    fixture::ensure(
        healthy.json["blockers"] == json!([]),
        format!("healthy blockers are wrong: {}", healthy.json["blockers"]),
    )?;
    fixture::ensure(
        healthy.json["disk"]["low_watermark_gb"] == 10,
        "the declared watermark is not reported",
    )?;
    fixture::ensure(
        healthy.json["disk"]["target_free_gb"] == 20
            && healthy.json["disk"]["policy_mode"] == "enforce",
        format!("disk policy report is wrong: {}", healthy.json["disk"]),
    )?;
    fixture::ensure(
        healthy.json["disk"]["free_gb"].is_number(),
        "the host did not report its free space",
    )?;
    fixture::ensure(
        healthy.json["capacity"]["accepting_jobs"] == true
            && healthy.json["capacity"]["available_cpu_cores"] == 2,
        format!("healthy capacity is wrong: {}", healthy.json["capacity"]),
    )?;
    fixture::ensure(
        healthy.json["capacity"]["published_at"].is_string(),
        "a live publication has no timestamp",
    )?;
    fixture::ensure(
        !healthy.json["capacity"]["age_seconds"].is_null(),
        "a live publication has no age",
    )?;

    let blocked_diag = json!({"disk_pressure_unresolved":true,"disk_cleanup_policy_known":true,"queue_paused":false,"pinned_only":false});
    fleet.publish_capacity(blocked_diag, 0, false, None)?;
    let blocked = gates(fleet)?;
    fixture::ensure(
        blocked.status != 0,
        "a host that is claiming nothing must not exit zero",
    )?;
    fixture::ensure(
        blocked.json["claiming"] == false
            && blocked.json["blockers"] == json!(["disk_pressure_unresolved"]),
        format!("blocked host report is wrong: {}", blocked.json),
    )?;
    fixture::ensure(
        blocked.json["capacity"]["available_cpu_cores"] == 0,
        format!("blocked host cores are wrong: {}", blocked.json["capacity"]),
    )?;
    let blocked_text = fleet.invoke(&["host", "gates", FIXTURE_HOST])?;
    fixture::ensure(blocked_text.status != 0, "blocked human report exited zero")?;
    fixture::ensure(
        blocked_text.output.contains("claiming: no"),
        format!(
            "blocked rendering omits claiming: no: {}",
            blocked_text.output
        ),
    )?;
    fixture::ensure(
        blocked_text
            .output
            .contains("blockers: disk_pressure_unresolved"),
        format!("blocked rendering omits blocker: {}", blocked_text.output),
    )?;
    fixture::ensure(
        blocked_text.output.contains(&format!(
            "{FIXTURE_HOST} is claiming nothing: disk_pressure_unresolved"
        )),
        "the failure message must name the host and its blockers",
    )?;

    let unknown_diag = json!({"disk_pressure_unresolved":false,"disk_cleanup_policy_known":false,"queue_paused":false,"pinned_only":false});
    fleet.publish_capacity(unknown_diag, 2, true, None)?;
    let policy_unknown = gates(fleet)?;
    fixture::ensure(
        policy_unknown.json["claiming"] == false,
        format!("unknown policy still claims: {}", policy_unknown.json),
    )?;
    fixture::ensure(
        fixture::array_contains_string(
            &policy_unknown.json["blockers"],
            "disk_cleanup_policy_unknown",
        ),
        format!("{}", policy_unknown.json["blockers"]),
    )?;
    let paused_diag = json!({"disk_pressure_unresolved":false,"disk_cleanup_policy_known":true,"queue_paused":true,"pinned_only":false});
    fleet.publish_capacity(paused_diag, 2, true, None)?;
    let paused = gates(fleet)?;
    fixture::ensure(
        paused.json["blockers"] == json!(["queue_paused"]),
        format!("paused blockers are wrong: {}", paused.json["blockers"]),
    )?;
    let stale_at = iso_timestamp(SystemTime::now() - Duration::from_secs(7_200));
    fleet.publish_capacity(claiming.clone(), 2, true, Some(stale_at))?;
    let stale = gates(fleet)?;
    fixture::ensure(
        stale.json["claiming"] == false,
        format!("stale host still claims: {}", stale.json),
    )?;
    fixture::ensure(
        fixture::array_contains_string(&stale.json["blockers"], "capacity_publication_stale"),
        format!("{}", stale.json["blockers"]),
    )?;
    fixture::ensure(
        stale.json["capacity"]["age_seconds"]
            .as_f64()
            .unwrap_or_default()
            > 3600.0,
        "the age of a stale publication is not reported",
    )?;
    let capacity = fleet
        .store
        .join(format!("capacity/local-{}.json", fixture::this_hostname()?));
    let _ = fs::remove_file(capacity);
    let silent = gates(fleet)?;
    fixture::ensure(
        silent.json["claiming"] == false,
        format!("silent host still claims: {}", silent.json),
    )?;
    fixture::ensure(
        fixture::array_contains_string(&silent.json["blockers"], "no_capacity_publication"),
        format!("{}", silent.json["blockers"]),
    )?;
    fixture::ensure(
        silent.json["capacity"]["published_at"].is_null(),
        format!("silent host has a publication time: {}", silent.json),
    )?;
    let silent_text = fleet.invoke(&["host", "gates", FIXTURE_HOST])?;
    fixture::ensure(
        silent_text
            .output
            .contains("nothing published for this host"),
        format!("silent rendering is wrong: {}", silent_text.output),
    )?;
    fleet.registry(&fixture::fixture_registry(
        json!({"disk_cleanup":disk_policy,"pinned_only":true}),
        None,
    )?)?;
    fleet.publish_capacity(claiming, 2, true, None)?;
    let pinned = gates(fleet)?;
    fixture::ensure(
        fixture::array_contains_string(&pinned.json["blockers"], "pinned_only"),
        format!("{}", pinned.json["blockers"]),
    )?;

    fixture::record_trace(context, "stado-host-gates", "host-gates", &fleet.binary, source.clone(), json!({
        "claiming":{"exit":healthy.status,"blockers":healthy.json["blockers"],"disk":healthy.json["disk"]},
        "diskPressure":{"exit":blocked.status,"blockers":blocked.json["blockers"]},
        "policyUnknown":policy_unknown.json["blockers"],"queuePaused":paused.json["blockers"],
        "stalePublication":{"blockers":stale.json["blockers"],"ageSeconds":stale.json["capacity"]["age_seconds"]},
        "noPublication":silent.json["blockers"],"pinnedOnly":pinned.json["blockers"]
    }), &[
        "a claiming host reports no blockers and exits zero",
        "a host that is claiming nothing exits non-zero and names the agent's own blocker words",
        "an unreadable disk policy is named separately from disk pressure",
        "a paused queue is reported as the agent published it",
        "a stale publication is reported with its age rather than dropped",
        "no publication at all is reported as no publication, not as an empty one",
        "a pinned host is reported as pinned",
        "the read is read-only: one df, one state read and one object read"
    ])
}

fn gates(fleet: &mut FleetFixture) -> Result<fixture::Invocation, String> {
    fleet.invoke_json(&["host", "gates", FIXTURE_HOST, "--json"])
}
