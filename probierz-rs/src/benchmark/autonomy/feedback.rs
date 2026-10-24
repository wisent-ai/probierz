//! What went wrong for the people using our products becomes the products'
//! work without anyone carrying it over: every open incident in Probierz's
//! failure register is one roadmap item of the catalog product its envelope
//! names as the service. Once the register holds the incident resolved, the
//! item stays in the roadmap as delivered with that state and its time as the
//! decision, so the roadmap keeps what was asked of the product. An incident
//! reopened after its item was delivered is planned as a new item, titled
//! with when the register recorded the reopening, and the delivered item keeps
//! its decision. An item whose incident the register does not hold resolved is
//! never delivered: that item alone is refused, naming it, and the rest of the
//! product's change is written. The register is
//! read through `probierz incident list`, the same answer an operator reads,
//! and every item is written through the catalog's one roadmap writer.

use std::path::Path;

use serde_json::{json, Value as Json};

use super::steps;
use crate::benchmark::record::catalog;

const PREFIX: &str = "Repair incident ";
/// What follows an incident's title in the item of an incident reopened after
/// its earlier item was delivered: when the register recorded the reopening.
const REOPENED: &str = ", reopened ";
/// The register's state of an incident nobody has resolved.
const OPEN: &str = "open";

/// The incident a `Repair incident` item is about.
fn incident_of(item: &Json) -> Option<&str> {
    let rest = item["title"].as_str()?.strip_prefix(PREFIX)?;
    Some(match rest.split_once(REOPENED) {
        Some((incident, _)) => incident,
        None => rest,
    })
}

/// The title of a new item for the open incident `row`: the incident's own
/// title when no item of it exists yet, and one naming its latest reopening
/// when every earlier item of it was delivered.
fn new_title(row: &Json, incident: &str, delivered_before: bool) -> Result<String, String> {
    if !delivered_before {
        return Ok(format!("{PREFIX}{incident}"));
    }
    match row["reopenings"]
        .as_array()
        .and_then(|reopenings| reopenings.last())
        .and_then(|reopening| reopening["reopened_at"].as_str())
    {
        Some(at) => Ok(format!("{PREFIX}{incident}{REOPENED}{at}")),
        None => Err(format!(
            "incident {incident} is open again after its roadmap item was delivered, but the register records no reopening of it to name the new item by"
        )),
    }
}

/// The decision that ends the item of `incident`: the state the register
/// holds it in and when it was resolved. An incident the register does not
/// list, or holds open, or resolved without its time, ends no item.
fn decision(rows: &[Json], incident: &str) -> Result<String, String> {
    let Some(row) = rows.iter().find(|row| row["incident_id"] == incident) else {
        return Err(format!(
            "the roadmap item of incident {incident} stays open: probierz incident list --state all does not list incident {incident}"
        ));
    };
    let state = row["state"].as_str();
    let resolved_at = row["resolution"]["resolved_at"].as_str();
    match (state, resolved_at) {
        (Some(state), Some(at)) if state != OPEN => {
            Ok(format!("probierz incident show {incident} reads {state} at {at}"))
        }
        _ => Err(format!(
            "the roadmap item of incident {incident} stays open: the register holds it in state {} with resolution time {}, not resolved",
            row["state"], row["resolution"]["resolved_at"]
        )),
    }
}

/// What one product's roadmap gains, and what of it the register cannot
/// name: the open incidents it has no open item for, planned; the open items
/// whose incident the register holds resolved, delivered; and, for each
/// incident or item that cannot be named from the register, the sentence
/// saying so. One unnamed incident refuses only its own item.
struct Changes {
    added: Vec<Json>,
    delivered: Vec<Json>,
    refused: Vec<String>,
}

/// The planned item of the open incident `row`, or why it cannot be named.
fn planned_item(row: &Json, incident: &str, items: &[Json]) -> Result<Json, String> {
    let delivered_before = items.iter().any(|item| incident_of(item) == Some(incident));
    let title = new_title(row, incident, delivered_before)?;
    if items.iter().any(|item| item["title"] == title.as_str()) {
        return Err(format!(
            "incident {incident} is open again, but its latest recorded reopening names the item {title:?}, which the roadmap already holds; the register records no newer reopening"
        ));
    }
    let Some(claim) = row["claim"].as_str() else {
        return Err(format!(
            "open incident {incident} records no claim to state the item's outcome by"
        ));
    };
    Ok(json!({
        "title": title,
        "status": catalog::PLANNED,
        "outcome": format!("probierz incident show {incident} reads resolved: {claim}"),
        "source": format!("probierz incident {incident}"),
    }))
}

fn changes_of(rows: &[Json], open: &[&Json], items: &[Json]) -> Changes {
    let mut changes = Changes {
        added: Vec::new(),
        delivered: Vec::new(),
        refused: Vec::new(),
    };
    let existing: Vec<&Json> = items.iter().filter(|item| catalog::is_open(item)).collect();
    let mut incidents = Vec::new();
    for row in open {
        let Some(incident) = row["incident_id"].as_str() else {
            changes
                .refused
                .push(format!("an open incident of this product states no incident_id: {row}"));
            continue;
        };
        incidents.push(incident);
        if existing.iter().any(|item| incident_of(item) == Some(incident)) {
            continue;
        }
        match planned_item(row, incident, items) {
            Ok(item) => changes.added.push(item),
            Err(refusal) => changes.refused.push(refusal),
        }
    }
    for item in &existing {
        let Some(incident) = incident_of(item) else {
            continue;
        };
        if incidents.contains(&incident) {
            continue;
        }
        match decision(rows, incident) {
            Ok(decision) => changes.delivered.push(catalog::delivered(item, &decision)),
            Err(refusal) => changes.refused.push(refusal),
        }
    }
    changes
}

/// Bring every product's incident items in line with the register.
pub(super) fn incidents(harness: &Path, me: &Path, products: &[Json]) -> Json {
    let listed = steps::run(
        me,
        &steps::args(&["incident", "list", "--state", "all", "--json"]),
        harness,
    );
    if !steps::ok(&listed) {
        return json!({"read": listed});
    }
    // Read as an empty list, a malformed answer withdrew every incident item
    // from every product's roadmap.
    let Some(rows) = listed["answer"]["incidents"].as_array() else {
        return json!({"read": listed, "refusal": "the incident list answer holds no incidents list"});
    };
    let mut changes = Vec::new();
    for product in products {
        let Some(id) = product["id"].as_str() else {
            changes.push(json!({"product": null, "refusal": format!("a catalog product states no id: {product}")}));
            continue;
        };
        let open: Vec<&Json> = rows
            .iter()
            .filter(|row| row["state"] == OPEN && row["envelope"]["service"] == id)
            .collect();
        let items = match catalog::titled(id, product, PREFIX) {
            Ok(items) => items,
            Err(failure) => {
                changes.push(json!({"product": id, "refusal": failure.detail}));
                continue;
            }
        };
        let Changes {
            added,
            delivered,
            refused,
        } = changes_of(rows, &open, &items);
        if added.is_empty() && delivered.is_empty() && refused.is_empty() {
            continue;
        }
        let written: Vec<Json> = added.iter().chain(&delivered).cloned().collect();
        let refusal = match catalog::write_roadmap(id, &written) {
            Ok(()) => Json::Null,
            Err(failure) => json!(failure.detail),
        };
        let delivered_titles: Vec<&Json> = delivered.iter().map(|item| &item["title"]).collect();
        changes.push(json!({
            "product": id,
            "added": added,
            "delivered": delivered_titles,
            "refused": refused,
            "written": refusal.is_null(),
            "refusal": refusal,
        }));
    }
    json!({"incidents": rows.len(), "changes": changes})
}
