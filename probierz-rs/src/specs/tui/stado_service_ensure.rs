use std::fs;

use serde_json::{json, Value};

use crate::specs;

use super::stado_fleet_fixture::{self as fixture, FleetFixture, FIXTURE_HOST};

const NAME: &str = "probierz-fixture-ensure";
const LABEL: &str = "com.wisent.compute.service.probierz-fixture-ensure";

pub fn run(context: &specs::Context) -> Result<(), String> {
    let source = fixture::source_identity()?;
    let mut fleet = FleetFixture::open(context, "stado-service-ensure")?;
    let result = run_fixture(context, &source, &mut fleet);
    fixture::bootout_agent(LABEL);
    let close = fleet.close();
    result.and(close)
}

fn run_fixture(
    context: &specs::Context,
    source: &Value,
    fleet: &mut FleetFixture,
) -> Result<(), String> {
    fleet.registry(&fixture::fixture_registry(json!({}), None)?)?;
    let program_dir = fleet.services_root.join(NAME).join("bin");
    let agents = fleet.home.join("Library/LaunchAgents");
    fs::create_dir_all(&program_dir).map_err(|error| error.to_string())?;
    fs::create_dir_all(&agents).map_err(|error| error.to_string())?;
    let program = fixture::compile_idle_program(&fleet.dir, &program_dir.join(NAME))?;
    let program_text = program.to_string_lossy();
    let plist_path = agents.join(format!("{LABEL}.plist"));

    let refused = fleet.invoke(&[
        "service",
        "ensure",
        NAME,
        "--host",
        FIXTURE_HOST,
        "--from",
        &program_text,
    ])?;
    fixture::ensure(
        refused.status != 0,
        "ensure without --reason must be refused",
    )?;
    fixture::ensure(
        refused.output.contains("--reason <REASON>"),
        format!("reasonless refusal is wrong: {}", refused.output),
    )?;
    fixture::ensure(
        !plist_path.exists(),
        "a refused ensure installed a unit anyway",
    )?;
    fixture::ensure(
        fixture::launchd_pid(LABEL)?.is_none(),
        "a refused ensure started a job anyway",
    )?;

    let first_reason = "the fixture host must run this program; first pass installs the unit";
    let created = fleet.invoke_json(&[
        "service",
        "ensure",
        NAME,
        "--host",
        FIXTURE_HOST,
        "--from",
        &program_text,
        "--reason",
        first_reason,
        "--json",
    ])?;
    fixture::ensure(
        created.status == 0,
        format!("the creating pass reported failure: {}", created.output),
    )?;
    fixture::ensure(
        created.json["action"] == "created"
            && created.json["name"] == NAME
            && created.json["label"] == LABEL,
        format!("creating answer is wrong: {}", created.json),
    )?;
    fixture::ensure(
        plist_path.exists(),
        "the unit file was not installed in the fixture HOME",
    )?;
    let live_pid = fixture::launchd_pid(LABEL)?.ok_or_else(|| {
        "launchd is running no process under the label ensure created".to_string()
    })?;
    fixture::ensure(
        fixture::value_u64(&created.json["pid"]) == Some(live_pid as u64),
        "the reported pid is not the pid launchd holds",
    )?;
    let unit_before = fs::metadata(&plist_path)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| error.to_string())?;

    let again = fleet.invoke_json(&[
        "service",
        "ensure",
        NAME,
        "--host",
        FIXTURE_HOST,
        "--from",
        &program_text,
        "--reason",
        "second pass must change nothing",
        "--json",
    ])?;
    fixture::ensure(
        again.status == 0,
        format!("the idempotent pass reported failure: {}", again.output),
    )?;
    fixture::ensure(
        again.json["action"] == "already_correct",
        "a host already running the program was not reported as correct",
    )?;
    fixture::ensure(
        fixture::value_u64(&again.json["pid"]) == Some(live_pid as u64),
        "the idempotent pass restarted the unit",
    )?;
    fixture::ensure(
        fixture::launchd_pid(LABEL)? == Some(live_pid),
        "the unit is running a different process after the second pass",
    )?;
    let unit_after = fs::metadata(&plist_path)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| error.to_string())?;
    fixture::ensure(
        unit_after == unit_before,
        "the idempotent pass rewrote the unit file",
    )?;

    let missing_program = program_dir.join("no-such-program");
    let missing_text = missing_program.to_string_lossy();
    let missing = fleet.invoke(&[
        "service",
        "ensure",
        "probierz-fixture-absent",
        "--host",
        FIXTURE_HOST,
        "--from",
        &missing_text,
        "--reason",
        "a program the host does not have",
    ])?;
    fixture::ensure(
        missing.status != 0,
        "ensuring an absent program must not succeed",
    )?;
    fixture::ensure(
        fixture::launchd_pid("com.wisent.compute.service.probierz-fixture-absent")?.is_none(),
        "an absent program still started a unit",
    )?;
    let registry = fleet.read_registry()?;
    let declared = registry["targets"][0]["services"]
        .as_array()
        .and_then(|services| services.iter().find(|service| service["label"] == LABEL))
        .ok_or_else(|| "the ensured unit was not recorded as a managed service".to_string())?;
    fixture::ensure(
        declared["path"].as_str() == Some(plist_path.to_string_lossy().as_ref()),
        format!("recorded service path is wrong: {declared}"),
    )?;

    fixture::record_trace(
        context,
        "stado-service-ensure",
        "service-ensure",
        &fleet.binary,
        source.clone(),
        json!({
            "refusedWithoutReason":{"exit":refused.status,"unitInstalled":plist_path.exists()},
            "created":{"action":created.json["action"],"pid":created.json["pid"]},
            "secondPass":{"action":again.json["action"],"pid":again.json["pid"],"unitRewritten":false},
            "recordedService":declared
        }),
        &[
            "ensure without --reason is refused and installs nothing",
            "the creating pass exits zero, reports created, and launchd holds the pid it reports",
            "the second pass reports already_correct with the same pid",
            "the second pass does not rewrite the unit file and does not restart the job",
            "an absent program is refused and no unit is installed for it",
            "the unit ensure installed is recorded as a managed service in the registry",
        ],
    )
}
