//! What went wrong for the people using our products becomes the products'
//! work without anyone carrying it over: every open incident in Probierz's
//! failure register is one roadmap item of the catalog product its envelope
//! names as the service, and the item is withdrawn once the incident is
//! resolved. The register is read through `probierz incident list`, the same
//! answer an operator reads.

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value as Json};

use super::steps;
use crate::benchmark::record::catalog;
use crate::stado::STADO_BIN;

const PREFIX: &str = "Repair incident ";

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
    let rows: Vec<Json> = listed["answer"]["incidents"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut changes = Vec::new();
    for product in products {
        let id = product["id"].as_str().unwrap_or_default();
        let open: Vec<&Json> = rows
            .iter()
            .filter(|row| row["state"] == "open" && row["envelope"]["service"] == id)
            .collect();
        let wanted: Vec<String> = open
            .iter()
            .map(|row| {
                format!(
                    "{PREFIX}{}",
                    row["incident_id"].as_str().unwrap_or_default()
                )
            })
            .collect();
        let existing: Vec<String> = product["roadmap"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["title"].as_str())
            .filter(|title| title.starts_with(PREFIX))
            .map(str::to_string)
            .collect();
        let mut added = Vec::new();
        for (row, title) in open.iter().zip(wanted.iter()) {
            if existing.contains(title) {
                continue;
            }
            let incident = row["incident_id"].as_str().unwrap_or_default();
            added.push(json!({
                "title": title,
                "status": "planned",
                "outcome": format!(
                    "probierz incident show {incident} reads resolved: {}",
                    row["claim"].as_str().unwrap_or_default()
                ),
                "source": format!("probierz incident {incident}"),
            }));
        }
        let removed: Vec<String> = existing
            .iter()
            .filter(|title| !wanted.contains(title))
            .cloned()
            .collect();
        if added.is_empty() && removed.is_empty() {
            continue;
        }
        let mut write = Command::new(STADO_BIN);
        write.args(["product", "registry", "set", id]);
        for item in &added {
            write.arg("--add-roadmap").arg(item.to_string());
        }
        for title in &removed {
            write.arg("--remove-roadmap").arg(title);
        }
        let refusal = catalog::stado(&mut write)
            .err()
            .map(|failure| failure.detail);
        changes.push(json!({
            "product": id,
            "added": added,
            "removed": removed,
            "written": refusal.is_none(),
            "refusal": refusal,
        }));
    }
    json!({"incidents": rows.len(), "changes": changes})
}
