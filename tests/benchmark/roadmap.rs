//! `probierz benchmark run` and `probierz benchmark roadmap` through the real
//! binary and the real Stado, against a catalog of one fictional product in a
//! workspace of the test's own (`WISENT_WORKSPACE`, `HOME`), so no
//! operator's catalog, harness or run history is read or written.
//!
//! A run in which our contender loses a case plans the case's roadmap item; a
//! newer run in which ours wins it records the same item as delivered, with
//! that run's id as its decision; a run that loses the case again leaves the
//! delivered item as it was and plans an item of its own named after that
//! run. The catalog `stado product catalog --json` serves is read back after
//! each step. A roadmap asked for before any run is refused and changes
//! nothing. The incident leg (`roadmap/incidents.rs`) does the same for the
//! items `benchmark cycle` writes from Probierz's failure register. Required:
//! `STADO_BIN`, the Stado executable under test.

#[path = "roadmap/incidents.rs"]
mod incidents;
#[path = "roadmap/world.rs"]
mod world;

use serde_json::json;

use world::{answered, manifest, probierz, roadmap, the_item, APP, CASE, CATALOG, ITEM, SUITE};

#[test]
fn a_won_case_stays_in_the_roadmap_as_delivered_with_its_run() {
    let world = world::world("won-case");
    manifest(&world, "wrong");

    let lost = answered(
        &probierz(&world, &["benchmark", "run", APP, "--suite", SUITE]),
        "the losing run",
    );
    let Some(losses) = lost["standing"]["losses"].as_array() else {
        panic!("the losing run's standing lists no losses: {lost}");
    };
    assert!(
        losses.iter().any(|loss| loss["case"] == CASE),
        "our contender should have lost {CASE}: {lost}"
    );
    answered(
        &probierz(&world, &["benchmark", "roadmap", APP, "--suite", SUITE]),
        "the roadmap after the losing run",
    );
    let planned = the_item(&roadmap(&world), ITEM);
    assert_eq!(planned["status"], "planned", "{planned}");
    assert!(
        planned.get("decision").is_none(),
        "an open item carries no decision: {planned}"
    );

    manifest(&world, "right");
    let won = answered(
        &probierz(
            &world,
            &[
                "benchmark",
                "run",
                APP,
                "--suite",
                SUITE,
                "--contender",
                "ours",
            ],
        ),
        "the winning run",
    );
    let Some(run_id) = won["runId"].as_str() else {
        panic!("the winning run names no runId: {won}");
    };
    let synced = answered(
        &probierz(&world, &["benchmark", "roadmap", APP, "--suite", SUITE]),
        "the roadmap after the winning run",
    );
    assert_eq!(synced["runId"], run_id, "{synced}");
    assert_eq!(synced["delivered"], json!([ITEM]), "{synced}");

    let delivered = the_item(&roadmap(&world), ITEM);
    assert_eq!(delivered["status"], "delivered", "{delivered}");
    assert_eq!(
        delivered["decision"], run_id,
        "the decision is the run that won the case: {delivered}"
    );

    // Lost again: the delivered item keeps its run, and the return is planned
    // as an item of its own named after the run that lost it.
    manifest(&world, "wrong");
    let lost_again = answered(
        &probierz(&world, &["benchmark", "run", APP, "--suite", SUITE]),
        "the run that loses the case again",
    );
    let Some(losing_run) = lost_again["runId"].as_str() else {
        panic!("the run that loses again names no runId: {lost_again}");
    };
    answered(
        &probierz(&world, &["benchmark", "roadmap", APP, "--suite", SUITE]),
        "the roadmap after the case was lost again",
    );
    let history = roadmap(&world);
    let kept = the_item(&history, ITEM);
    assert_eq!(
        kept["status"], "delivered",
        "the delivered item stays delivered: {kept}"
    );
    assert_eq!(
        kept["decision"], run_id,
        "the delivered item keeps its run: {kept}"
    );
    let returned = the_item(&history, &format!("{ITEM}, lost again in run {losing_run}"));
    assert_eq!(returned["status"], "planned", "{returned}");
    assert!(
        returned.get("decision").is_none(),
        "an open item carries no decision: {returned}"
    );
}

#[test]
fn a_roadmap_before_any_run_is_refused_and_writes_nothing() {
    let world = world::world("no-run");
    manifest(&world, "wrong");
    let catalog = world.workspace.join(CATALOG);
    let before = std::fs::read(&catalog).expect("read the catalog");

    let refused = probierz(&world, &["benchmark", "roadmap", APP, "--suite", SUITE]);
    assert!(!refused.status.success(), "a roadmap with no run passed");
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        said.contains(&format!("no run of suite {SUITE} is recorded for {APP}")),
        "the refusal names the missing run: {said}"
    );
    assert_eq!(
        std::fs::read(&catalog).expect("read the catalog"),
        before,
        "a refused roadmap changed the catalog"
    );
}
