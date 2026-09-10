use std::fs;
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};

use crate::failure::iso_timestamp;
use crate::specs;

use super::fleet_fixture::{self as fixture, FleetFixture, FIXTURE_HOST, FIXTURE_PRODUCT};

const DESIRED: &str = "0.2.27";
const QUARANTINE_REASON: &str = "candidate did not become ready within 90s: pid 46748 is gone";

pub fn run(context: &specs::Context) -> Result<(), String> {
    let source = fixture::source_identity()?;
    let mut fleet = FleetFixture::open(context, "stado-release-quarantine")?;
    let result = run_fixture(context, &source, &mut fleet);
    let close = fleet.close();
    result.and(close)
}

fn run_fixture(
    context: &specs::Context,
    source: &Value,
    fleet: &mut FleetFixture,
) -> Result<(), String> {
    let digest = "a".repeat(64);
    let install_root = fleet.services_root.join(FIXTURE_PRODUCT);
    let state_path = fleet.state_dir.join(format!("{FIXTURE_PRODUCT}.json"));
    let audit_path = fleet
        .state_dir
        .join(format!("{FIXTURE_PRODUCT}.quarantine-audit.jsonl"));
    let quarantined_at = iso_timestamp(SystemTime::now() - Duration::from_secs(3_600));
    let release = fixture::fixture_release_control(
        &fleet.home,
        &fleet.state_dir,
        &fleet.logs_root,
        DESIRED,
        &digest,
        &install_root,
    );
    fleet.registry(&fixture::fixture_registry(json!({}), Some(release))?)?;
    fleet.publish_capacity(json!({"disk_pressure_unresolved":false,"disk_cleanup_policy_known":true,"queue_paused":false,"pinned_only":false}), 2, true, None)?;
    let unreconciled = fleet.invoke_json(&[
        "release",
        "quarantine",
        "list",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
        "--json",
    ])?;
    fixture::ensure(
        unreconciled.status == 0,
        format!(
            "listing an unreconciled host failed: {}",
            unreconciled.output
        ),
    )?;
    fixture::ensure(
        unreconciled.json["entries"] == json!([]),
        format!("unreconciled entries are wrong: {}", unreconciled.json),
    )?;
    let unreconciled_text = fleet.invoke(&[
        "release",
        "quarantine",
        "list",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
    ])?;
    fixture::ensure(
        unreconciled_text
            .output
            .contains(state_path.to_string_lossy().as_ref()),
        "an absent state file must be reported by the path the agent would have written",
    )?;

    let mut state = fixture::settled_state("0.2.26", &"d".repeat(64), &install_root.join("0.2.26"));
    state["phase"] = json!("quarantined");
    state["detail"] = json!(QUARANTINE_REASON);
    state["quarantined"] =
        json!({digest.clone():{"reason":QUARANTINE_REASON,"quarantined_at":quarantined_at}});
    fleet.write_release_state(&state)?;
    let state_before = fs::read_to_string(&state_path).map_err(|error| error.to_string())?;
    let listed = fleet.invoke_json(&[
        "release",
        "quarantine",
        "list",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
        "--json",
    ])?;
    fixture::ensure(
        listed.status == 0,
        format!("quarantine list failed: {}", listed.output),
    )?;
    fixture::ensure(
        listed.json["product"] == FIXTURE_PRODUCT && listed.json["target"] == FIXTURE_HOST,
        format!("quarantine list identity is wrong: {}", listed.json),
    )?;
    fixture::ensure(
        listed.json["entries"].as_array().map(Vec::len) == Some(1),
        format!("quarantine list entries are wrong: {}", listed.json),
    )?;
    fixture::ensure(
        listed.json["entries"][0]["digest"] == digest
            && listed.json["entries"][0]["reason"] == QUARANTINE_REASON,
        format!("quarantine entry is wrong: {}", listed.json["entries"][0]),
    )?;
    fixture::ensure(
        listed.json["entries"][0]["is_desired_digest"] == true,
        "the entry blocking the current rollout must be called out",
    )?;

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
                &digest,
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
                &digest,
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
                &digest,
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

    let reason = "stderr named a missing config key; fixed and republished in 0.2.28";
    let cleared = fleet.invoke_json(&[
        "release",
        "quarantine",
        "clear",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
        "--digest",
        &digest,
        "--reason",
        reason,
        "--json",
    ])?;
    fixture::ensure(
        cleared.status == 0,
        format!("clearing failed: {}", cleared.output),
    )?;
    fixture::ensure(
        cleared.json["digest"] == digest
            && cleared.json["cleared"] == true
            && cleared.json["reason"] == reason,
        format!("clear answer is wrong: {}", cleared.json),
    )?;
    fixture::ensure(
        cleared.json["audited_at"].is_string(),
        "the clear reports no audit instant",
    )?;
    let backup = cleared.json["state_backup"]
        .as_str()
        .ok_or_else(|| "the clear reports no state backup".to_string())?;
    let after: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    fixture::ensure(
        after["quarantined"] == json!({}),
        format!("quarantine was not emptied: {after}"),
    )?;
    fixture::ensure(
        after["phase"] == state["phase"]
            && after["rollout_generation"] == state["rollout_generation"]
            && after["active"] == state["active"],
        "clear changed rollout fields other than quarantined",
    )?;
    fixture::ensure(
        fs::read_to_string(backup).map_err(|error| error.to_string())? == state_before,
        "the backup is not the state that was replaced",
    )?;
    let audit_text = fs::read_to_string(&audit_path).map_err(|error| error.to_string())?;
    let audit_lines = audit_text.trim().lines().collect::<Vec<_>>();
    fixture::ensure(
        audit_lines.len() == 1,
        format!("audit has {} lines instead of one", audit_lines.len()),
    )?;
    let audit: Value = serde_json::from_str(audit_lines[0]).map_err(|error| error.to_string())?;
    fixture::ensure(
        audit["host"] == FIXTURE_HOST
            && audit["product"] == FIXTURE_PRODUCT
            && audit["digest"] == digest
            && audit["reason"] == reason
            && audit["quarantine_reason"] == QUARANTINE_REASON,
        format!("audit line is wrong: {audit}"),
    )?;
    let original =
        chrono::DateTime::parse_from_rfc3339(&quarantined_at).map_err(|error| error.to_string())?;
    let audited_original =
        chrono::DateTime::parse_from_rfc3339(audit["quarantined_at"].as_str().unwrap_or_default())
            .map_err(|error| error.to_string())?;
    fixture::ensure(
        original == audited_original,
        "audit changed the quarantine instant",
    )?;
    fixture::ensure(
        audit["state_backup"] == backup,
        format!("audit names the wrong backup: {audit}"),
    )?;
    fixture::ensure(audit["actor"].is_string(), "the audit line names no actor")?;
    fixture::ensure(
        audit["audited_at"].is_string(),
        "the audit line carries no instant",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fixture::ensure(
            fs::metadata(&audit_path)
                .map_err(|error| error.to_string())?
                .permissions()
                .mode()
                & 0o777
                == 0o600,
            "the audit trail is not owner-only",
        )?;
    }
    let again = fleet.invoke(&[
        "release",
        "quarantine",
        "clear",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
        "--digest",
        &digest,
        "--reason",
        "already cleared; this pass must refuse",
    ])?;
    fixture::ensure(
        again.status != 0,
        "clearing a digest that is no longer quarantined must be refused",
    )?;
    let doctor = fleet.invoke_json(&[
        "release",
        "doctor",
        FIXTURE_PRODUCT,
        "--target",
        FIXTURE_HOST,
        "--json",
    ])?;
    fixture::ensure(
        !fixture::array_contains_string(&doctor.json["blockers"], "desired_digest_quarantined"),
        "the desired digest is still reported as quarantined after a clear",
    )?;
    fixture::ensure(
        doctor.json["quarantined"] == json!([]),
        "the quarantine map is not empty after a clear",
    )?;

    fixture::record_trace(context, "stado-release-quarantine", "release-quarantine", &fleet.binary, source.clone(), json!({
        "refusalStatuses":refusal_statuses,"listedEntry":listed.json["entries"][0],
        "clear":{"cleared":cleared.json["cleared"],"stateBackup":backup,"auditPath":audit_path,"auditActor":audit["actor"]},
        "doctorBlockersAfterClear":doctor.json["blockers"]
    }), &[
        "clear is refused without --digest, without --target, without --reason and with a blank reason",
        "a refused clear leaves the rollout state byte-identical and writes no file",
        "clear removes exactly the named entry and changes no other rollout field",
        "the previous state is copied to a timestamped backup before the rewrite",
        "one owner-only audit line records actor, reason and the quarantine entry it destroyed",
        "clearing a digest that is not quarantined is refused",
        "after a clear the desired digest is no longer a doctor blocker"
    ])
}
