//! The incident leg of the roadmap journey: real `probierz incident` commands
//! fill the register, the real `probierz benchmark cycle` writes the product's
//! incident items through the roadmap writer, and the real Stado catalog is
//! read back after each cycle. The cycle runs on an evidence directory whose
//! one topic is no Stado identity, so it scouts nothing and only the incident
//! feedback writes.

use serde_json::{json, Value};

use super::world::{answered, probierz, roadmap, stado_run, the_item, world, World, APP};

/// The repository's own policy, scouting only the evidence directory.
fn policy(world: &World) {
    let own = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../autonomy.yaml"),
    )
    .expect("the repository's own policy");
    std::fs::write(
        world.harness.join("autonomy.yaml"),
        format!("{own}evidenceDir: evidence\n"),
    )
    .expect("write the policy");
    let evidence = world.harness.join("evidence");
    std::fs::create_dir_all(&evidence).expect("create the evidence directory");
    std::fs::write(evidence.join("Not-A-Stado-Identity.json"), "{}\n")
        .expect("write the evidence topic");
}

fn record(world: &World, claim: &str) -> String {
    let recorded = answered(
        &probierz(
            world,
            &[
                "incident", "record", "--claim", claim, "--service", APP, "--failure-point",
                "roadmap.journey", "--code", "journey_failure", "--detail", claim, "--json",
            ],
        ),
        "incident record",
    );
    let Some(id) = recorded["incident_id"].as_str() else {
        panic!("the recorded incident names no incident_id: {recorded}");
    };
    id.to_string()
}

fn show(world: &World, id: &str) -> Value {
    answered(&probierz(world, &["incident", "show", id, "--json"]), "incident show")
}

fn cycle(world: &World) -> Value {
    answered(&probierz(world, &["benchmark", "cycle"]), "benchmark cycle")
}

/// The refusals the cycle reported for items of `alpha`'s roadmap, one per
/// item it could not name, joined; the change itself was written.
fn refusal(cycled: &Value) -> String {
    let Some(change) = cycled["roadmapChanges"]
        .as_array()
        .and_then(|changes| changes.iter().find(|change| change["product"] == APP))
    else {
        panic!("the cycle reported no roadmap change of {APP}: {cycled}");
    };
    assert_eq!(change["written"], true, "{APP}'s roadmap change was not written: {cycled}");
    let Some(refused) = change["refused"].as_array().filter(|refused| !refused.is_empty()) else {
        panic!("no item of {APP}'s roadmap was refused: {cycled}");
    };
    let said: Vec<String> = refused.iter().map(|sentence| sentence.to_string()).collect();
    said.join("; ")
}

/// An item written into `alpha`'s roadmap through the real Stado registry.
fn add_item(world: &World, item: Value) {
    answered(
        &stado_run(
            world,
            &["product", "registry", "set", APP, "--add-roadmap", &item.to_string(), "--json"],
        ),
        "stado product registry set",
    );
}

#[test]
fn an_incident_is_planned_delivered_with_its_resolution_and_planned_again_when_reopened() {
    let world = world("incident-lifecycle");
    policy(&world);
    let id = record(&world, "alpha answers the journey");
    let title = format!("Repair incident {id}");

    cycle(&world);
    let planned = the_item(&roadmap(&world), &title);
    assert_eq!(planned["status"], "planned", "{planned}");

    answered(
        &probierz(&world, &["incident", "resolve", &id, "--note", "the journey answers"]),
        "incident resolve",
    );
    let Some(resolved_at) = show(&world, &id)["resolution"]["resolved_at"].as_str().map(str::to_string) else {
        panic!("the resolved incident states no resolved_at");
    };
    cycle(&world);
    let delivered = the_item(&roadmap(&world), &title);
    assert_eq!(delivered["status"], "delivered", "{delivered}");
    let decision = format!("probierz incident show {id} reads resolved at {resolved_at}");
    assert_eq!(delivered["decision"], decision.as_str(), "{delivered}");

    answered(
        &probierz(&world, &["incident", "reopen", &id, "--note", "the journey fails again"]),
        "incident reopen",
    );
    let shown = show(&world, &id);
    let Some(reopened_at) = shown["reopenings"]
        .as_array()
        .and_then(|reopenings| reopenings.last())
        .and_then(|reopening| reopening["reopened_at"].as_str())
    else {
        panic!("the reopened incident records no reopening: {shown}");
    };
    cycle(&world);
    let history = roadmap(&world);
    let kept = the_item(&history, &title);
    assert_eq!(kept["status"], "delivered", "the delivered item stays delivered: {kept}");
    assert_eq!(kept["decision"], decision.as_str(), "the delivered item keeps its decision: {kept}");
    let reopened = the_item(&history, &format!("{title}, reopened {reopened_at}"));
    assert_eq!(reopened["status"], "planned", "{reopened}");
}

#[test]
fn an_item_whose_incident_the_register_does_not_list_is_refused_alone() {
    let world = world("incident-unlisted");
    policy(&world);
    let ghost = "Repair incident not-in-the-register";
    add_item(
        &world,
        json!({"title": ghost, "status": "planned", "outcome": "the register resolves it", "source": "this journey"}),
    );
    let id = record(&world, "alpha answers the journey");

    let said = refusal(&cycle(&world));
    assert!(
        said.contains("does not list incident not-in-the-register"),
        "the refusal names the incident the register lacks: {said}"
    );
    let history = roadmap(&world);
    assert_eq!(the_item(&history, ghost)["status"], "planned", "the unnamed item stays open");
    assert_eq!(
        the_item(&history, &format!("Repair incident {id}"))["status"],
        "planned",
        "the open incident is planned in the same cycle: {history:?}"
    );
}

#[test]
fn a_reopened_incident_without_a_recorded_reopening_is_refused() {
    let world = world("incident-unrecorded-reopening");
    policy(&world);
    let id = record(&world, "alpha answers the journey");
    let title = format!("Repair incident {id}");
    add_item(
        &world,
        json!({"title": title, "status": "delivered", "outcome": "the register resolves it", "source": "this journey", "decision": "this journey"}),
    );

    let said = refusal(&cycle(&world));
    assert!(
        said.contains("records no reopening"),
        "the refusal names the missing reopening: {said}"
    );
    let history = roadmap(&world);
    assert_eq!(
        history.iter().filter(|item| item["title"].as_str().is_some_and(|written| written.starts_with(&title))).count(),
        history.iter().filter(|item| item["title"] == title.as_str()).count(),
        "no new item was planned for the unnamed return: {history:?}"
    );
}
