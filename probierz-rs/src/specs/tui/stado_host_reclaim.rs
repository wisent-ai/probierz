use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use crate::specs;

use super::stado_fleet_fixture::{self as fixture, FleetFixture, FIXTURE_HOST, FIXTURE_PRODUCT};

struct Tree {
    tree: PathBuf,
    file: PathBuf,
}

pub fn run(context: &specs::Context) -> Result<(), String> {
    let source = fixture::source_identity()?;
    let mut fleet = FleetFixture::open(context, "stado-host-reclaim")?;
    let mut held_pid = None;
    let result = run_fixture(context, &source, &mut fleet, &mut held_pid);
    fixture::stop(held_pid);
    let close = fleet.close();
    result.and(close)
}

fn run_fixture(
    context: &specs::Context,
    source: &Value,
    fleet: &mut FleetFixture,
    held_pid: &mut Option<u32>,
) -> Result<(), String> {
    fleet.registry(&fixture::fixture_registry(json!({}), None)?)?;
    let host_bin = fleet.home.join(".stado/bin");
    fs::create_dir_all(&host_bin).map_err(|error| error.to_string())?;
    let installed = Command::new("/bin/cp")
        .arg(&fleet.binary)
        .arg(host_bin.join("stado"))
        .output()
        .map_err(|error| error.to_string())?;
    fixture::ensure(
        installed.status.success(),
        "could not place a stado binary in the fixture home",
    )?;
    let scratch = fleet.home.join(".stado/build-work/stado");
    fs::create_dir_all(&scratch).map_err(|error| error.to_string())?;
    let scratch_file = scratch.join("vendor.tar");
    fs::write(&scratch_file, "x".repeat(4096)).map_err(|error| error.to_string())?;
    age(&scratch_file)?;
    age(&scratch)?;
    let reclaimable = delivered_tree(fleet, FIXTURE_PRODUCT, "0.2.20", true)?;
    let held = delivered_tree(fleet, FIXTURE_PRODUCT, "0.2.21", true)?;
    let linked = delivered_tree(fleet, FIXTURE_PRODUCT, "0.2.24", true)?;
    let newest = delivered_tree(fleet, FIXTURE_PRODUCT, "0.2.26", false)?;
    let current = fleet.services_root.join(FIXTURE_PRODUCT).join("current");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&linked.tree, &current).map_err(|error| error.to_string())?;
    fixture::ensure(current.exists(), "the fixture has no current link")?;
    *held_pid = Some(fixture::spawn_orphan(&format!(
        "/bin/sh {}",
        fixture::shell_quote(held.file.to_string_lossy().as_ref())
    ))?);
    let pid = held_pid.expect("held pid");

    let refused = fleet.invoke(&["host", "reclaim", FIXTURE_HOST, "--apply"])?;
    fixture::ensure(
        refused.status != 0,
        "--apply without --reason must be refused",
    )?;
    fixture::ensure(
        refused
            .output
            .contains("--apply removes files and needs --reason"),
        format!("apply refusal is wrong: {}", refused.output),
    )?;
    fixture::ensure(
        refused
            .output
            .contains("appended to the host's own audit log"),
        format!("apply refusal omits audit explanation: {}", refused.output),
    )?;
    fixture::ensure(
        !refused.output.contains("STAGE"),
        "a refused apply must not have measured the stages",
    )?;
    let preview = fleet.invoke_json(&["host", "reclaim", FIXTURE_HOST, "--json"])?;
    fixture::ensure(
        preview.status == 0,
        format!("the preview failed: {}", preview.output),
    )?;
    fixture::ensure(
        preview.json["host"] == FIXTURE_HOST && preview.json["mode"] == "dry_run",
        format!("preview identity is wrong: {}", preview.json),
    )?;
    let stages = preview.json["stages"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| row["stage"].clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    fixture::ensure(
        stages
            == vec![
                json!("registry_cleanup"),
                json!("build_scratch"),
                json!("delivered_trees"),
            ],
        "the three declared stages are not all reported",
    )?;
    fixture::ensure(
        preview.json["free_gb_before"].is_number() && preview.json["free_gb_after"].is_number(),
        format!("preview free-space values are wrong: {}", preview.json),
    )?;
    let rendered = fleet.invoke(&["host", "reclaim", FIXTURE_HOST])?;
    fixture::ensure(
        rendered.status == 0,
        format!("the human preview failed: {}", rendered.output),
    )?;
    let banner = rendered
        .output
        .find("DRY RUN")
        .ok_or_else(|| "the preview does not say it is a preview".to_string())?;
    let table = rendered.output.find("STAGE").unwrap_or(usize::MAX);
    fixture::ensure(banner < table, "the preview banner comes after the table")?;
    let nothing =
        regex::Regex::new(r"nothing on .* is deleted").map_err(|error| error.to_string())?;
    fixture::ensure(
        nothing.is_match(&rendered.output),
        format!("preview deletion promise is missing: {}", rendered.output),
    )?;
    fixture::ensure(
        rendered.output.contains("--apply --reason"),
        format!("preview omits apply instruction: {}", rendered.output),
    )?;
    fixture::ensure(
        !rendered.output.contains("APPLIED"),
        "a preview must not report an apply",
    )?;
    fixture::ensure(
        rendered.output.contains(scratch.to_string_lossy().as_ref()),
        "the stale build scratch tree was not named",
    )?;
    fixture::ensure(
        rendered
            .output
            .contains(reclaimable.tree.to_string_lossy().as_ref()),
        "the stale delivered tree was not named",
    )?;
    fixture::ensure(
        !rendered
            .output
            .contains(newest.tree.to_string_lossy().as_ref()),
        "the newest delivered tree must never be named",
    )?;
    fixture::ensure(
        !rendered
            .output
            .contains(linked.tree.to_string_lossy().as_ref()),
        "the tree `current` resolves to must never be named",
    )?;
    fixture::ensure(
        !rendered
            .output
            .contains(held.tree.to_string_lossy().as_ref()),
        "a tree a live process holds must never be named",
    )?;
    for path in [
        &scratch,
        &scratch_file,
        &reclaimable.tree,
        &reclaimable.file,
        &held.tree,
        &linked.tree,
        &newest.tree,
        &current,
    ] {
        fixture::ensure(
            path.exists(),
            format!("the preview deleted {}", path.display()),
        )?;
    }
    fixture::ensure(
        fixture::alive(pid),
        "the preview killed the process holding a delivered tree",
    )?;

    fixture::record_trace(context, "stado-host-reclaim", "host-reclaim", &fleet.binary, source.clone(), json!({
        "applyWithoutReason":{"exit":refused.status},
        "preview":{"mode":preview.json["mode"],"stages":preview.json["stages"],"namedBuildScratch":scratch,"namedDeliveredTree":reclaimable.tree},
        "guards":{"newestTreeKept":newest.tree,"currentTargetKept":linked.tree,"heldTreeKept":held.tree,"heldByPidStillRunning":pid}
    }), &[
        "--apply without --reason is refused before any stage is measured",
        "the preview is the default and reports all three declared stages",
        "the preview says it is a preview before it prints what it would remove",
        "a stale build scratch tree and a stale delivered tree are named",
        "the newest delivered tree, the current link target and a tree a live process holds are never named",
        "the preview deletes nothing and signals nothing"
    ])
}

fn delivered_tree(
    fleet: &FleetFixture,
    product: &str,
    version: &str,
    stale: bool,
) -> Result<Tree, String> {
    let tree = fleet.services_root.join(product).join(version);
    let bin = tree.join("bin");
    fs::create_dir_all(&bin).map_err(|error| error.to_string())?;
    let file = bin.join("start");
    fs::write(
        &file,
        format!("#!/bin/sh\n# {product} {version}\nwhile :; do /bin/sleep 5; done\n"),
    )
    .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
    }
    if stale {
        age(&file)?;
        age(&bin)?;
        age(&tree)?;
    }
    Ok(Tree { tree, file })
}

fn age(path: &Path) -> Result<(), String> {
    let output = Command::new("/usr/bin/touch")
        .args(["-t", "202001010000.00"])
        .arg(path)
        .output()
        .map_err(|error| error.to_string())?;
    fixture::ensure(
        output.status.success(),
        format!(
            "could not age {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        ),
    )
}
