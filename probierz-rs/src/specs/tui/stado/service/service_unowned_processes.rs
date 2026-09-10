use std::fs;

use serde_json::{json, Value};

use crate::specs;

use crate::specs::tui::stado::fleet_fixture::{self as fixture, FleetFixture, FIXTURE_HOST, FIXTURE_PRODUCT};

const LABEL: &str = "ai.wisent.probierz.fixture.owned";

pub fn run(context: &specs::Context) -> Result<(), String> {
    let source = fixture::source_identity()?;
    let mut fleet = FleetFixture::open(context, "stado-service-unowned-processes")?;
    let mut orphan = None;
    let mut reader = None;
    let result = run_fixture(context, &source, &mut fleet, &mut orphan, &mut reader);
    fixture::bootout_agent(LABEL);
    fixture::stop(orphan);
    fixture::stop(reader);
    let close = fleet.close();
    result.and(close)
}

fn run_fixture(
    context: &specs::Context,
    source: &Value,
    fleet: &mut FleetFixture,
    orphan: &mut Option<u32>,
    reader: &mut Option<u32>,
) -> Result<(), String> {
    let plist_path = fleet
        .home
        .join(format!("Library/LaunchAgents/{LABEL}.plist"));
    fleet.registry(&fixture::fixture_registry(json!({"services":[{"name":"probierz-fixture-owned","kind":"launchd","label":LABEL,"unit":"","path":plist_path,"managed_since":"2026-08-18T00:00:00Z"}]}), None)?)?;
    let tree = fleet.services_root.join(FIXTURE_PRODUCT).join("0.2.26/bin");
    fs::create_dir_all(&tree).map_err(|error| error.to_string())?;
    let entry = tree.join("start");
    fs::write(&entry, "#!/bin/sh\nwhile :; do /bin/sleep 5; done\n")
        .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
    }
    let log_file = fleet
        .services_root
        .join(FIXTURE_PRODUCT)
        .join("0.2.26/run.log");
    fs::write(&log_file, "a log under the managed root\n").map_err(|error| error.to_string())?;
    *orphan = Some(fixture::spawn_orphan(&format!(
        "/bin/sh {}",
        fixture::shell_quote(entry.to_string_lossy().as_ref())
    ))?);
    fs::create_dir_all(plist_path.parent().expect("plist parent"))
        .map_err(|error| error.to_string())?;
    let owned_program = fixture::compile_idle_program(&fleet.dir, &tree.join("owned-daemon"))?;
    fixture::write_agent_plist(&plist_path, LABEL, &[&owned_program])?;
    let owned_pid = fixture::bootstrap_agent(&plist_path, LABEL)?;
    *reader = Some(fixture::spawn_orphan(&format!(
        "/usr/bin/tail -f {}",
        fixture::shell_quote(log_file.to_string_lossy().as_ref())
    ))?);
    let orphan_pid = orphan.expect("orphan pid");
    let reader_pid = reader.expect("reader pid");

    let listed = fleet.invoke_json(&["service", "list", "--unowned", "--json"])?;
    fixture::ensure(
        listed.status == 0,
        format!("listing unowned processes failed: {}", listed.output),
    )?;
    let rows = listed.json["unowned"]
        .as_array()
        .ok_or_else(|| "unowned report has no array".to_string())?;
    let pids = rows
        .iter()
        .filter_map(|row| fixture::value_u64(&row["pid"]))
        .map(|pid| pid as u32)
        .collect::<Vec<_>>();
    fixture::ensure(
        pids.contains(&orphan_pid),
        format!("the unowned product process {orphan_pid} was not reported"),
    )?;
    fixture::ensure(
        !pids.contains(&owned_pid),
        format!("a process launchd owns ({owned_pid}) must not be reported"),
    )?;
    fixture::ensure(
        !pids.contains(&reader_pid),
        "a reader that merely names the root must not be reported",
    )?;
    let reported = rows
        .iter()
        .find(|row| fixture::value_u64(&row["pid"]) == Some(orphan_pid as u64))
        .ok_or_else(|| format!("the unowned product process {orphan_pid} was not reported"))?;
    fixture::ensure(
        reported["host"] == FIXTURE_HOST,
        format!("reported host is wrong: {reported}"),
    )?;
    fixture::ensure(
        reported["command"]
            .as_str()
            .map(|command| command.contains(entry.to_string_lossy().as_ref()))
            .unwrap_or(false),
        "the report does not name what the process is running",
    )?;
    fixture::ensure(
        reported["started_at"].is_string(),
        "the report does not say when the process started",
    )?;
    fixture::ensure(
        reported["product_guess"] == FIXTURE_PRODUCT,
        "the report does not attribute the process to the product whose root it runs from",
    )?;
    let rendered = fleet.invoke(&["service", "list", "--unowned"])?;
    fixture::ensure(
        rendered.status == 0,
        format!("human unowned list failed: {}", rendered.output),
    )?;
    fixture::ensure(
        rendered.output.contains(&orphan_pid.to_string()),
        "the human rendering omits the unowned pid",
    )?;
    fixture::ensure(
        fixture::alive(orphan_pid),
        "the read killed the unowned process",
    )?;
    fixture::ensure(
        fixture::alive(owned_pid),
        "the read killed the owned process",
    )?;

    fixture::record_trace(context, "stado-service-unowned-processes", "service-unowned-processes", &fleet.binary, source.clone(), json!({
        "reported":reported,"ownedPidExcluded":owned_pid,"readerPidExcluded":reader_pid,"unownedCount":rows.len()
    }), &[
        "a product process no launchd job owns is reported with pid, command, start time and product",
        "a process launchd owns is not reported, even under the same managed root",
        "a reader that merely names a path under the root is not reported",
        "the read signals nothing: every process it enumerated is still running afterwards"
    ])
}
