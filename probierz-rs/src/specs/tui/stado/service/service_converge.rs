use std::fs;

use serde_json::{json, Value};

use crate::specs;

use crate::specs::tui::stado::fleet_fixture::{self as fixture, FleetFixture, FIXTURE_HOST};

const LABEL: &str = "ai.wisent.probierz.fixture.converge";
const BINARY: &str = "fixture-daemon";
const DECLARED_VERSION: &str = "1.0.0";

pub fn run(context: &specs::Context) -> Result<(), String> {
    let source = fixture::source_identity()?;
    let mut fleet = FleetFixture::open(context, "stado-service-converge")?;
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
    let artefact_root = fleet.home.join(BINARY);
    let plist_path = fleet
        .home
        .join(format!("Library/LaunchAgents/{LABEL}.plist"));
    let host_bin = fleet.home.join(".stado/bin");
    fs::create_dir_all(&host_bin).map_err(|error| format!("{}: {error}", host_bin.display()))?;
    let installed_stado = host_bin.join("stado");
    fs::copy(&fleet.binary, &installed_stado)
        .map_err(|error| format!("{} -> {}: {error}", fleet.binary, installed_stado.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&installed_stado, fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(artefact_root.join("bin")).map_err(|error| error.to_string())?;
    fs::create_dir_all(plist_path.parent().expect("plist parent"))
        .map_err(|error| error.to_string())?;
    fs::write(
        artefact_root.join("package.json"),
        format!("{{\n  \"name\": \"{BINARY}\",\n  \"version\": \"{DECLARED_VERSION}\"\n}}\n"),
    )
    .map_err(|error| error.to_string())?;
    let declared_program =
        fixture::compile_idle_program(&fleet.dir, &artefact_root.join(format!("bin/{BINARY}")))?;
    let relinked_program = fixture::copy_program(
        &declared_program,
        &artefact_root.join(format!("bin/{BINARY}-relinked")),
    )?;

    declare(fleet, &plist_path, DECLARED_VERSION, false)?;
    let without = fleet.invoke_json(&["service", "converge", FIXTURE_HOST, "--json"])?;
    fixture::ensure(
        without.status == 0,
        format!("converge failed with no unit declared: {}", without.output),
    )?;
    let unitless = row(&without.json)?;
    fixture::ensure(
        unitless["declared_version"] == DECLARED_VERSION
            && unitless["installed_version"] == DECLARED_VERSION
            && unitless["verdict"] == "in-sync",
        format!("unitless version report is wrong: {unitless}"),
    )?;
    fixture::ensure(
        unitless["running_binary"].is_null(),
        format!("unitless row names a process: {unitless}"),
    )?;
    fixture::ensure(
        unitless["binary_matches_process"].is_null(),
        "an unasked process question must not answer true",
    )?;

    fixture::write_agent_plist(&plist_path, LABEL, &[&declared_program])?;
    let pid = fixture::bootstrap_agent(&plist_path, LABEL)?;
    declare(fleet, &plist_path, DECLARED_VERSION, true)?;
    let matching = fleet.invoke_json(&["service", "converge", FIXTURE_HOST, "--json"])?;
    fixture::ensure(
        matching.status == 0,
        format!("converge failed on a matching host: {}", matching.output),
    )?;
    let matched = row(&matching.json)?;
    fixture::ensure(
        matched["unit"] == LABEL && matched["state"] == "running",
        format!("matching unit report is wrong: {matched}"),
    )?;
    fixture::ensure(
        matched["running_binary"].as_str() == Some(declared_program.to_string_lossy().as_ref()),
        format!("matching running binary is wrong: {matched}"),
    )?;
    fixture::ensure(
        matched["binary_matches_process"] == true && matched["verdict"] == "in-sync",
        format!("matching process verdict is wrong: {matched}"),
    )?;

    fixture::write_agent_plist(&plist_path, LABEL, &[&relinked_program])?;
    let differing = fleet.invoke_json(&["service", "converge", FIXTURE_HOST, "--json"])?;
    fixture::ensure(
        differing.status == 0,
        "a process mismatch is a finding on an in-sync host, not a failed command",
    )?;
    let differs = row(&differing.json)?;
    fixture::ensure(
        differs["binary_matches_process"] == false,
        "the live process is not the declared artefact",
    )?;
    fixture::ensure(
        differs["running_binary"].as_str() == Some(declared_program.to_string_lossy().as_ref()),
        "the report must name what the process actually executes",
    )?;
    fixture::ensure(
        differs["installed_version"] == DECLARED_VERSION,
        format!("mismatch installed version is wrong: {differs}"),
    )?;
    fixture::ensure(
        differs["verdict"] == "in-sync",
        "the version comparison is independent of the process comparison",
    )?;
    let text = fleet.invoke(&["service", "converge", FIXTURE_HOST])?;
    fixture::ensure(
        text.output.contains("PROCESS"),
        format!("human table omits PROCESS: {}", text.output),
    )?;
    fixture::ensure(
        text.output.contains("differs"),
        "the human table does not say the process differs",
    )?;

    declare(fleet, &plist_path, "9.9.9", true)?;
    let drifted = fleet.invoke_json(&["service", "converge", FIXTURE_HOST, "--json"])?;
    fixture::ensure(
        drifted.status != 0,
        "a drifted host must fail the report-mode gate",
    )?;
    let drift = row(&drifted.json)?;
    fixture::ensure(
        drift["declared_version"] == "9.9.9"
            && drift["installed_version"] == DECLARED_VERSION
            && drift["verdict"] == "drifted",
        format!("drift report is wrong: {drift}"),
    )?;
    fixture::ensure(
        drifted.json["applied"] == false && drifted.json["releases"] == json!([]),
        format!("report-mode converge applied work: {}", drifted.json),
    )?;
    fixture::ensure(
        drifted.json["undeliverable"].as_array().is_some(),
        format!("undeliverable is not an array: {}", drifted.json),
    )?;
    fixture::ensure(
        fixture::launchd_pid(LABEL)? == Some(pid),
        "a report-mode converge restarted the unit",
    )?;
    let unknown = fleet.invoke(&["service", "converge", FIXTURE_HOST, "not-declared"])?;
    fixture::ensure(unknown.status != 0, "an undeclared binary was accepted")?;
    fixture::ensure(
        unknown.output.contains("declares no not-declared version"),
        format!("unknown binary refusal is wrong: {}", unknown.output),
    )?;
    let registry = fleet.read_registry()?;
    fixture::ensure(
        registry["targets"][0]["managed_versions"][BINARY] == "9.9.9",
        "converge rewrote the registry declaration",
    )?;

    fixture::record_trace(context, "stado-service-converge", "service-converge", &fleet.binary, source.clone(), json!({
        "noUnit":{"verdict":unitless["verdict"],"runningBinary":unitless["running_binary"],"matches":unitless["binary_matches_process"]},
        "matching":{"runningBinary":matched["running_binary"],"matches":matched["binary_matches_process"],"verdict":matched["verdict"]},
        "relinkedUnderLiveJob":{"runningBinary":differs["running_binary"],"matches":differs["binary_matches_process"],"verdict":differs["verdict"]},
        "drift":{"exit":drifted.status,"verdict":drift["verdict"],"applied":drifted.json["applied"],"releases":drifted.json["releases"]},"pidUnchanged":pid
    }), &[
        "a declared binary with no unit reports null process fields, never a match",
        "binary_matches_process is true when the live process executes the declared artefact",
        "binary_matches_process is false when it does not, while the version verdict stays in-sync",
        "the human table renders that mismatch as differs",
        "a drifted host exits non-zero in report mode and delivers nothing",
        "a report-mode pass restarts nothing and never edits the registry declaration",
        "a binary the target does not declare is refused before the host is contacted"
    ])
}

fn declare(
    fleet: &FleetFixture,
    plist: &std::path::Path,
    version: &str,
    with_unit: bool,
) -> Result<(), String> {
    let mut target = json!({"managed_versions":{BINARY:version}});
    if with_unit {
        target["services"] = json!([{"name":BINARY,"kind":"launchd","label":LABEL,"unit":"","path":plist,"managed_since":"2026-08-18T00:00:00Z"}]);
    }
    fleet.registry(&fixture::fixture_registry(target, None)?)
}

fn row(report: &Value) -> Result<&Value, String> {
    let rows = report["binaries"]
        .as_array()
        .ok_or_else(|| "converge report has no binaries array".to_string())?;
    fixture::ensure(rows.len() == 1, "the fixture declares exactly one binary")?;
    Ok(&rows[0])
}
