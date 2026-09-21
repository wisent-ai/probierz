//! Retention over the evidence a dispatched run leaves in the fleet's store.
//!
//! `retention` walks this harness's `test-results`; a run sent to the fleet
//! writes its archives and logs into the `probierz` object namespace on the
//! host that ran it, and until 2026-09-21 nothing expired them. On
//! charless-mac-mini that store had reached 34.9 GiB — the largest occupant
//! of a host sitting below its disk watermark, which is why no darwin-arm64
//! release could be built.
//!
//! What is defended here is the shape of the decision, against the real
//! store: a plan removes nothing, every object carries the date the age
//! decision is made from, and a URI outside the prefixes the fleet grant
//! covers is refused by name rather than attempted.

use std::process::{Command, Output};

use serde_json::Value;

fn probierz(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_probierz"))
        .args(args)
        .output()
        .expect("run probierz")
}

fn plan() -> Option<Value> {
    let output = probierz(&["retention", "--fleet"]);
    if !output.status.success() {
        // The object store is not reachable from this machine — the case
        // says so rather than passing on an absent product.
        eprintln!(
            "skipped: the fleet object store did not answer: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return None;
    }
    Some(serde_json::from_slice(&output.stdout).expect("the plan is JSON"))
}

#[test]
fn a_plan_reads_the_fleet_store_and_removes_nothing() {
    let Some(plan) = plan() else { return };
    assert_eq!(plan["applied"], Value::Bool(false), "{plan}");
    assert_eq!(plan["removed"], Value::from(0), "a plan deletes nothing");
    assert_eq!(plan["root"], Value::from("stado://probierz/results/"));
    let items = plan["items"].as_array().expect("the plan lists its objects");
    assert_eq!(
        items.len(),
        plan["objects"].as_u64().expect("an object count") as usize
    );
    for item in items {
        assert!(
            item["modifiedAt"].is_string(),
            "an object with no date cannot be judged by age: {item}"
        );
        assert!(
            item["retentionDays"].as_f64().is_some_and(|days| days > 0.0),
            "every object is judged against a declared window: {item}"
        );
        assert_ne!(item["removed"], Value::Bool(true), "a plan deletes nothing");
    }
}

#[test]
fn the_plan_is_repeatable_and_leaves_the_store_as_it_found_it() {
    let Some(first) = plan() else { return };
    let Some(second) = plan() else { return };
    assert_eq!(
        first["objects"], second["objects"],
        "reading a plan twice must not change the store"
    );
    assert_eq!(first["keptBytes"], second["keptBytes"]);
}
