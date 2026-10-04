//! The benchmark against the product catalog Stado serves.
//!
//! The catalog says who a product's rivals are and which Probierz suites
//! measure them. `rivals` confronts that declaration with the manifest: every
//! rival must have a contender and every named suite must be declared, or the
//! declaration is a promise nothing keeps. `roadmap` turns the newest run's
//! losses into the product's roadmap, one item per case, and withdraws the
//! item for a case once a newer run shows ours winning it.

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value as Json};

use super::commands::standing_of;
use crate::benchmark::inputs::declare::declared;
use crate::failure::{ended, print_json, Answer, Code, Failure};
use crate::manifest::{self, Manifest};
use crate::stado::STADO_BIN;

/// The catalog id of the product a manifest describes.
pub(crate) fn product_id(manifest: &Manifest) -> String {
    match manifest
        .document
        .get("productId")
        .and_then(serde_yaml::Value::as_str)
    {
        Some(id) => id.to_string(),
        None => manifest.app_id.clone(),
    }
}

/// Run one Stado command and answer its stdout, or say what it refused.
pub(crate) fn stado(command: &mut Command) -> Result<Vec<u8>, Failure> {
    let described = format!("{command:?}");
    let output = command.output().map_err(|error| {
        Failure::new(
            "benchmark.catalog",
            Code::Prerequisite,
            format!("{described} could not be started ({error}); install Stado, which serves the product catalog"),
        )
    })?;
    if !output.status.success() {
        // Stado's sentence first: the failure line is cut for a person, and a
        // long argv ahead of it hid the reason.
        let verb: Vec<String> = command
            .get_args()
            .take(2)
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        return Err(Failure::new(
            "benchmark.catalog",
            Code::Unavailable,
            format!(
                "{} (stado {} {})",
                String::from_utf8_lossy(&output.stderr).trim(),
                verb.join(" "),
                ended(&output.status),
            ),
        ));
    }
    Ok(output.stdout)
}

/// Every product record in the catalog Stado serves.
pub(crate) fn products() -> Result<Vec<Json>, Failure> {
    let mut read = Command::new(STADO_BIN);
    read.arg("product").arg("catalog").arg("--json");
    let mut catalog: Json = serde_json::from_slice(&stado(&mut read)?).map_err(|error| {
        Failure::config(
            "benchmark.catalog",
            format!("stado product catalog --json is not JSON: {error}"),
        )
    })?;
    match catalog["products"].take() {
        Json::Array(products) => Ok(products),
        _ => Err(Failure::config(
            "benchmark.catalog",
            "stado product catalog --json carries no products list",
        )),
    }
}

/// One product's record in the catalog Stado serves.
pub(crate) fn record(product: &str) -> Result<Json, Failure> {
    products()?
        .into_iter()
        .find(|entry| entry["id"] == product)
        .ok_or_else(|| {
            Failure::config(
                "benchmark.catalog",
                format!("the product catalog has no product {product}; the manifest's productId names it"),
            )
        })
}

pub(crate) fn rivals(harness: &Path, app_id: &str) -> Answer {
    let manifest = manifest::load(harness, app_id)?;
    let declared = declared(&manifest)?;
    let product = product_id(&manifest);
    let entry = record(&product)?;
    let empty = Vec::new();
    let named = entry["rivals"].as_array().unwrap_or(&empty);
    let rivals: Vec<Json> = named
        .iter()
        .map(|rival| {
            let id = rival["id"].as_str().unwrap_or_default();
            json!({
                "id": id,
                "name": rival["name"],
                "url": rival["url"],
                "measured": declared.contenders.get(id).is_some_and(|contender| !contender.ours),
            })
        })
        .collect();
    let suites: Vec<Json> = entry["benchmark"]["suites"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .map(|suite| {
            let id = suite.as_str().unwrap_or_default();
            json!({"id": id, "declared": declared.suites.contains_key(id)})
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
        gaps.push(format!(
            "rival {} has no contender in {}; probierz benchmark author {app_id} --contender {} --suite <id> drafts and verifies one",
            rival["id"],
            manifest.file.display(),
            rival["id"].as_str().unwrap_or_default()
        ));
    }
    for suite in suites.iter().filter(|suite| suite["declared"] == false) {
        gaps.push(format!(
            "suite {} is named by the catalog but not declared in {}; probierz benchmark author-suite {app_id} --suite {} drafts one",
            suite["id"],
            manifest.file.display(),
            suite["id"].as_str().unwrap_or_default()
        ));
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

fn title(suite: &str, case: &str) -> String {
    format!("Win benchmark case {suite}/{case}")
}

pub(crate) fn roadmap(harness: &Path, app_id: &str, suite_id: &str) -> Answer {
    print_json(&roadmap_of(harness, app_id, suite_id)?)
}

/// Bring the product's catalog roadmap in line with the newest run of one
/// suite and answer what was added and withdrawn.
pub(crate) fn roadmap_of(harness: &Path, app_id: &str, suite_id: &str) -> Result<Json, Failure> {
    let manifest = manifest::load(harness, app_id)?;
    let product = product_id(&manifest);
    let standing = standing_of(harness, app_id, suite_id)?;
    let run_id = standing["runId"].as_str().unwrap_or_default().to_string();
    let ours = standing["ours"].as_str().unwrap_or_default().to_string();
    let prefix = title(suite_id, "");
    let existing: Vec<String> = record(&product)?["roadmap"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["title"].as_str())
        .filter(|title| title.starts_with(&prefix))
        .map(str::to_string)
        .collect();
    let mut added = Vec::new();
    let mut wanted = Vec::new();
    for loss in standing["losses"].as_array().into_iter().flatten() {
        let case = loss["case"].as_str().unwrap_or_default();
        let winners: Vec<&str> = loss["winners"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
            .collect();
        added.push(json!({
            "title": title(suite_id, case),
            "status": "planned",
            "outcome": format!(
                "probierz benchmark standing {app_id} --suite {suite_id} shows {ours} winning or tying case {case}, which {} won",
                winners.join(", ")
            ),
            "source": format!("probierz benchmark run {run_id} of {app_id}"),
        }));
        wanted.push(title(suite_id, case));
    }
    let removed: Vec<String> = existing
        .into_iter()
        .filter(|old| !wanted.contains(old))
        .collect();
    if !added.is_empty() || !removed.is_empty() {
        let mut write = Command::new(STADO_BIN);
        write
            .arg("product")
            .arg("registry")
            .arg("set")
            .arg(&product);
        for item in &added {
            write.arg("--add-roadmap").arg(item.to_string());
        }
        for old in &removed {
            write.arg("--remove-roadmap").arg(old);
        }
        stado(&mut write)?;
    }
    let written = record(&product)?;
    let titles: Vec<&str> = written["roadmap"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["title"].as_str())
        .collect();
    let missing: Vec<&String> = wanted
        .iter()
        .filter(|title| !titles.contains(&title.as_str()))
        .collect();
    let lingering: Vec<&String> = removed
        .iter()
        .filter(|title| titles.contains(&title.as_str()))
        .collect();
    if !missing.is_empty() || !lingering.is_empty() {
        return Err(Failure::new(
            "benchmark.roadmap",
            Code::Unknown,
            format!(
                "the catalog read back after the write lacks {missing:?} and still holds {lingering:?}"
            ),
        ));
    }
    Ok(json!({
        "appId": app_id,
        "product": product,
        "runId": run_id,
        "added": added,
        "removed": removed,
    }))
}
