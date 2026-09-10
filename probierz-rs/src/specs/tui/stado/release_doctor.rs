use std::fs;
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};

use crate::failure::iso_timestamp;
use crate::specs;

use super::fleet_fixture::{self as fixture, FleetFixture, FIXTURE_HOST, FIXTURE_PRODUCT};

const DESIRED: &str = "0.2.27";

pub fn run(context: &specs::Context) -> Result<(), String> {
    let source = fixture::source_identity()?;
    let mut fleet = FleetFixture::open(context, "stado-release-doctor")?;
    let result = run_fixture(context, &source, &mut fleet);
    let close = fleet.close();
    result.and(close)
}

fn run_fixture(
    context: &specs::Context,
    source: &Value,
    fleet: &mut FleetFixture,
) -> Result<(), String> {
    let desired_digest = "a".repeat(64);
    let install_root = fleet.services_root.join(FIXTURE_PRODUCT);
    let disk = json!({"mode":"enforce","low_free_gb":10,"target_free_gb":20,"check_interval_seconds":300,"max_bytes_per_pass":21474836480u64,"max_items_per_pass":50,"max_scan_items":10000,"cleaners":{}});
    let release = fixture::fixture_release_control(
        &fleet.home,
        &fleet.state_dir,
        &fleet.logs_root,
        DESIRED,
        &desired_digest,
        &install_root,
    );
    fleet.registry(&fixture::fixture_registry(
        json!({"disk_cleanup":disk}),
        Some(release),
    )?)?;
    let healthy = json!({"disk_pressure_unresolved":false,"disk_cleanup_policy_known":true,"queue_paused":false,"pinned_only":false});
    fleet.publish_capacity(healthy.clone(), 2, true, None)?;
    fleet.write_release_state(&fixture::settled_state(
        DESIRED,
        &desired_digest,
        &install_root.join(DESIRED),
    ))?;
    let settled = diagnose(fleet)?;
    fixture::ensure(
        settled.status == 0,
        format!("doctor failed on a settled host: {}", settled.output),
    )?;
    fixture::ensure(
        settled.json["desired_version"] == DESIRED
            && settled.json["observed_version"] == DESIRED
            && settled.json["verdict"] == "settled",
        format!("settled report is wrong: {}", settled.json),
    )?;
    fixture::ensure(
        settled.json["blockers"] == json!([]) && settled.json["quarantined"] == json!([]),
        format!("settled blockers are wrong: {}", settled.json),
    )?;
    fixture::ensure(
        settled.json["gates"]["disk_pressure_unresolved"] == false,
        format!("settled gates are wrong: {}", settled.json["gates"]),
    )?;
    for key in [
        "product",
        "target",
        "desired_version",
        "observed_version",
        "phase",
        "detail",
        "candidate",
        "quarantined",
        "gates",
        "verdict",
        "blockers",
    ] {
        fixture::ensure(
            settled.json.get(key).is_some(),
            format!("the report is missing {key}"),
        )?;
    }
    for key in ["port", "health_status", "pid_alive"] {
        fixture::ensure(
            settled.json["candidate"].get(key).is_some(),
            format!("the candidate section is missing {key}"),
        )?;
    }
    fixture::ensure(
        settled.json["candidate"]["health_status"] == "no_candidate",
        format!("candidate health is wrong: {}", settled.json["candidate"]),
    )?;

    let quarantined_at = iso_timestamp(SystemTime::now() - Duration::from_secs(3_600));
    let mut blocked_state =
        fixture::settled_state("0.2.26", &"d".repeat(64), &install_root.join("0.2.26"));
    blocked_state["phase"] = json!("quarantined");
    blocked_state["detail"] = json!("candidate did not become ready within 90s: pid 46748 is gone");
    blocked_state["quarantined"] = json!({desired_digest.clone():{"reason":"candidate did not become ready within 90s: pid 46748 is gone","quarantined_at":quarantined_at}});
    fleet.write_release_state(&blocked_state)?;
    let state_path = fleet.state_dir.join(format!("{FIXTURE_PRODUCT}.json"));
    let state_before = fs::read_to_string(&state_path).map_err(|error| error.to_string())?;
    let blocked = diagnose(fleet)?;
    fixture::ensure(
        blocked.status == 0,
        "a blocked verdict is a finding, not a failed command",
    )?;
    fixture::ensure(
        blocked.json["observed_version"] == "0.2.26" && blocked.json["verdict"] == "blocked",
        format!("blocked verdict is wrong: {}", blocked.json),
    )?;
    fixture::ensure(
        fixture::array_contains_string(&blocked.json["blockers"], "desired_digest_quarantined"),
        format!(
            "the quarantined desired digest is not named: {}",
            blocked.json["blockers"]
        ),
    )?;
    fixture::ensure(
        blocked.json["quarantined"].as_array().map(Vec::len) == Some(1),
        format!(
            "quarantine report is wrong: {}",
            blocked.json["quarantined"]
        ),
    )?;
    fixture::ensure(
        blocked.json["quarantined"][0]["digest"] == desired_digest
            && blocked.json["quarantined"][0]["is_desired_digest"] == true,
        format!(
            "quarantined entry is wrong: {}",
            blocked.json["quarantined"][0]
        ),
    )?;
    let written_at =
        chrono::DateTime::parse_from_rfc3339(&quarantined_at).map_err(|error| error.to_string())?;
    let reported_at = chrono::DateTime::parse_from_rfc3339(
        blocked.json["quarantined"][0]["quarantined_at"]
            .as_str()
            .unwrap_or_default(),
    )
    .map_err(|error| error.to_string())?;
    fixture::ensure(
        written_at == reported_at,
        "quarantined instant changed in the report",
    )?;
    fixture::ensure(
        blocked.json["phase"] == "quarantined",
        format!("blocked phase is wrong: {}", blocked.json["phase"]),
    )?;
    fixture::ensure(
        blocked.json["detail"]
            .as_str()
            .map(|text| text.contains("pid 46748 is gone"))
            .unwrap_or(false),
        format!("blocked detail is wrong: {}", blocked.json["detail"]),
    )?;
    fixture::ensure(
        fs::read_to_string(&state_path).map_err(|error| error.to_string())? == state_before,
        "diagnosing rewrote the rollout state",
    )?;
    let rendered = fleet.invoke(&[
        "release",
        "doctor",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
    ])?;
    fixture::ensure(
        rendered.status == 0,
        format!("human doctor failed: {}", rendered.output),
    )?;
    let verdict = regex::Regex::new(r"verdict\s+blocked").map_err(|error| error.to_string())?;
    let blockers = regex::Regex::new(r"blockers\s+desired_digest_quarantined")
        .map_err(|error| error.to_string())?;
    fixture::ensure(
        verdict.is_match(&rendered.output),
        format!("human report omits blocked verdict: {}", rendered.output),
    )?;
    fixture::ensure(
        blockers.is_match(&rendered.output),
        format!("human report omits blocker: {}", rendered.output),
    )?;
    fixture::ensure(
        rendered
            .output
            .contains(&format!("next: stado release logs {FIXTURE_PRODUCT}")),
        format!("human report omits next command: {}", rendered.output),
    )?;

    let mut stale_state =
        fixture::settled_state(DESIRED, &desired_digest, &install_root.join(DESIRED));
    stale_state["quarantined"] = json!({"e".repeat(64):{"reason":"an older candidate never became ready","quarantined_at":quarantined_at}});
    fleet.write_release_state(&stale_state)?;
    let stale = diagnose(fleet)?;
    fixture::ensure(
        stale.json["verdict"] == "settled",
        "a stale quarantine entry must not block the current release",
    )?;
    fixture::ensure(
        stale.json["quarantined"][0]["is_desired_digest"] == false
            && stale.json["blockers"] == json!([]),
        format!("stale quarantine report is wrong: {}", stale.json),
    )?;
    let gated_diag = json!({"disk_pressure_unresolved":true,"disk_cleanup_policy_known":true,"queue_paused":false,"pinned_only":false});
    fleet.publish_capacity(gated_diag, 0, false, None)?;
    fleet.write_release_state(&fixture::settled_state(
        DESIRED,
        &desired_digest,
        &install_root.join(DESIRED),
    ))?;
    let gated = diagnose(fleet)?;
    fixture::ensure(
        gated.json["observed_version"] == DESIRED,
        "the host is at the desired version",
    )?;
    fixture::ensure(
        gated.json["verdict"] == "blocked",
        "an unresolved disk gate blocks a rollout at the desired version",
    )?;
    fixture::ensure(
        fixture::array_contains_string(&gated.json["blockers"], "disk_pressure_unresolved"),
        format!("{}", gated.json["blockers"]),
    )?;
    fixture::ensure(
        gated.json["gates"]["disk_pressure_unresolved"] == true,
        format!("gated report is wrong: {}", gated.json["gates"]),
    )?;
    fixture::ensure(
        gated.json["gates"]["low_watermark_gb"] == 10,
        "the gate is reported against the declared watermark",
    )?;

    fixture::record_trace(
        context,
        "stado-release-doctor",
        "release-doctor",
        &fleet.binary,
        source.clone(),
        json!({
            "settled":{"verdict":settled.json["verdict"],"observed":settled.json["observed_version"]},
            "quarantinedDesiredDigest":{"verdict":blocked.json["verdict"],"blockers":blocked.json["blockers"]},
            "staleQuarantine":{"verdict":stale.json["verdict"],"blockers":stale.json["blockers"]},
            "unresolvedDiskGate":{"verdict":gated.json["verdict"],"blockers":gated.json["blockers"]}
        }),
        &[
            "settled only when the version the host reports equals the desired one",
            "blocked when the desired artefact digest sits in the host quarantine map",
            "a quarantined digest that is not the desired one does not block",
            "blocked when the host disk gate is unresolved, even at the desired version",
            "the report carries the complete contracted key set including the candidate section",
            "diagnosing starts nothing and writes nothing back to the rollout state",
        ],
    )
}

fn diagnose(fleet: &mut FleetFixture) -> Result<fixture::Invocation, String> {
    fleet.invoke_json(&[
        "release",
        "doctor",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
        "--json",
    ])
}
