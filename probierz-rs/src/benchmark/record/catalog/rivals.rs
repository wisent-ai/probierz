//! `probierz benchmark rivals <app>`: the catalog's rivals and suites for a
//! product confronted with its manifest. Every rival must have a contender
//! and every named suite must be declared, or the declaration is a promise
//! nothing keeps.

use std::path::Path;

use serde_json::{json, Value as Json};

use super::{product_id, record};
use crate::benchmark::inputs::declare::declared;
use crate::failure::{print_json, Answer, Failure};
use crate::manifest;

/// The list `field` of `value`, read as none when Stado left the field out
/// (it drops a product's rivals and benchmark when there are none); a field
/// that is there and is not a list is refused naming it.
fn listed<'a>(value: &'a Json, field: &str, product: &str) -> Result<&'a [Json], Failure> {
    match value.get(field) {
        None => Ok(&[]),
        Some(Json::Array(items)) => Ok(items),
        Some(other) => Err(Failure::config(
            "benchmark.rivals",
            format!("{product}'s catalog {field} is {other}, not a list"),
        )),
    }
}

pub(crate) fn rivals(harness: &Path, app_id: &str) -> Answer {
    let manifest = manifest::load(harness, app_id)?;
    let declared = declared(&manifest)?;
    let product = product_id(&manifest);
    let entry = record(&product)?;
    let named = listed(&entry, "rivals", &product)?;
    let rivals: Vec<Json> = named
        .iter()
        .map(|rival| {
            let id = rival["id"].as_str();
            json!({
                "id": id,
                "name": rival["name"],
                "url": rival["url"],
                "measured": id.is_some_and(|id| declared.contenders.get(id).is_some_and(|contender| !contender.ours)),
            })
        })
        .collect();
    let named_suites = match entry.get("benchmark") {
        None => &[][..],
        Some(benchmark) => listed(benchmark, "suites", &product)?,
    };
    let suites: Vec<Json> = named_suites
        .iter()
        .map(|suite| {
            let id = suite.as_str();
            json!({"id": id, "declared": id.is_some_and(|id| declared.suites.contains_key(id))})
        })
        .collect();
    let undeclared: Vec<&str> = declared
        .contenders
        .values()
        .filter(|contender| {
            !contender.ours
                && !named
                    .iter()
                    .any(|rival| rival["id"] == contender.id.as_str())
        })
        .map(|contender| contender.id.as_str())
        .collect();
    let catalog_app = entry["benchmark"]["app"].as_str();
    let mut gaps = Vec::new();
    if named.is_empty() {
        gaps.push(format!(
            "the catalog names no rival for {product}; stado product registry set {product} --add-rival declares one"
        ));
    }
    if catalog_app != Some(app_id) {
        gaps.push(format!(
            "the catalog's benchmark for {product} names app {catalog_app:?}, not {app_id}"
        ));
    }
    for rival in rivals.iter().filter(|rival| rival["measured"] == false) {
        gaps.push(match rival["id"].as_str() {
            Some(id) => format!(
                "rival {id} has no contender in {}; probierz benchmark author contender {app_id} --contender {id} --suite <id> drafts and verifies one",
                manifest.file.display(),
            ),
            None => format!("a rival the catalog names for {product} states no id: {rival}"),
        });
    }
    for suite in suites.iter().filter(|suite| suite["declared"] == false) {
        gaps.push(match suite["id"].as_str() {
            Some(id) => format!(
                "suite {id} is named by the catalog but not declared in {}; probierz benchmark author suite {app_id} --suite {id} drafts one",
                manifest.file.display(),
            ),
            None => format!("a suite the catalog names for {product} is not a suite id: {suite}"),
        });
    }
    for id in &undeclared {
        gaps.push(format!(
            "contender {id} is not a rival the catalog names for {product}"
        ));
    }
    print_json(&json!({
        "appId": app_id,
        "product": product,
        "rivals": rivals,
        "suites": suites,
        "gaps": gaps,
    }))?;
    if gaps.is_empty() {
        Ok(())
    } else {
        Err(Failure::config(
            "benchmark.rivals",
            format!(
                "{} gap(s) between the catalog and the manifest: {}",
                gaps.len(),
                gaps.join("; ")
            ),
        ))
    }
}
