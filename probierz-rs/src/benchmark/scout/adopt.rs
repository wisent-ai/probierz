//! The operator's decision on one scouted brief. Stado creates the private
//! repository, its canonical checkout and the preview catalog record; the
//! catalog then names the brief's rivals and the benchmark that measures
//! them; Probierz declares the product's manifest over the new checkout and
//! drafts its first suite. Every step is idempotent, so an adoption that
//! stopped part way is finished by running it again.

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value as Json};

use super::BRIEF_SCHEMA;
use crate::benchmark::author;
use crate::benchmark::author::place;
use crate::benchmark::record::catalog::{self, stado};
use crate::failure::{print_json, Answer, Code, Failure};
use crate::manifest;
use crate::stado::STADO_BIN;

const POINT: &str = "benchmark.adopt";

fn read_brief(file: &Path) -> Result<Json, Failure> {
    let text = std::fs::read_to_string(file).map_err(|error| {
        Failure::invalid(POINT, format!("{} cannot be read: {error}", file.display()))
    })?;
    let brief: Json = serde_json::from_str(&text).map_err(|error| {
        Failure::invalid(POINT, format!("{} is not JSON: {error}", file.display()))
    })?;
    if brief["schema"] != BRIEF_SCHEMA {
        return Err(Failure::invalid(
            POINT,
            format!(
                "{} is not a scouted brief ({BRIEF_SCHEMA}); probierz benchmark scout writes one",
                file.display()
            ),
        ));
    }
    Ok(brief)
}

/// The manifest of the new product over the checkout Stado made, written
/// only when there is none, and loaded through the ordinary manifest judge.
fn manifest_of(harness: &Path, id: &str, checkout: &str) -> Result<manifest::Manifest, Failure> {
    let file = manifest::apps_root(harness).join(id).join("probierz.yaml");
    if !file.exists() {
        let root = match std::env::var("HOME").ok().and_then(|home| {
            Path::new(checkout)
                .strip_prefix(home)
                .ok()
                .map(Path::to_path_buf)
        }) {
            Some(relative) => format!("~/{}", relative.display()),
            None => checkout.to_string(),
        };
        std::fs::create_dir_all(file.parent().expect("a manifest sits in its app directory"))?;
        std::fs::write(
            &file,
            format!("schemaVersion: 1\nappId: {id}\nproductId: {id}\nowner: {id} maintainers\nrepositories:\n  - root: {root}\n    mappings: []\nsurfaces: {{}}\njourneys: {{}}\n"),
        )?;
    }
    manifest::load(harness, id)
}

pub(crate) fn adopt(
    harness: &Path,
    brief_file: &Path,
    allow_create: bool,
    cases: usize,
    rounds: u32,
) -> Answer {
    let brief = read_brief(brief_file)?;
    let creation = &brief["creation"];
    let id = creation["product"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if !allow_create {
        return Err(Failure::invalid(
            POINT,
            format!(
                "adopting {} creates the private repositories {} through Stado and a preview catalog record for {id}; --allow-create is the operator's authority for that",
                brief_file.display(),
                creation["repositories"]
            ),
        ));
    }
    let request = brief_file.with_extension("creation.json");
    std::fs::write(&request, serde_json::to_string_pretty(creation)? + "\n")?;
    let created: Json = serde_json::from_slice(&stado(
        Command::new(STADO_BIN)
            .args(["product", "create", "--allow-create", "--json", "--request"])
            .arg(&request),
    )?)
    .map_err(|error| {
        Failure::config(
            POINT,
            format!("stado product create answered no JSON: {error}"),
        )
    })?;
    if created["state"] != "provisioned" {
        print_json(&json!({"brief": brief_file, "creation": created}))?;
        return Err(Failure::new(
            POINT,
            Code::Unavailable,
            format!("stado product create left {id} {}: {}; `stado product create --resume {} --allow-create` continues it", created["state"], created["error"], creation["request_id"].as_str().unwrap_or_default()),
        ));
    }
    let checkout = created["checkouts"][0]["path"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let manifest = manifest_of(harness, &id, &checkout)?;
    let mut declare = Command::new(STADO_BIN);
    declare.args(["product", "registry", "set", &id]);
    for rival in brief["rivals"].as_array().into_iter().flatten() {
        declare.arg("--add-rival").arg(rival.to_string());
    }
    declare
        .arg("--benchmark")
        .arg(brief["benchmark"].to_string());
    stado(&mut declare)?;
    let record = catalog::record(&id)?;
    let suite_id = brief["benchmark"]["suites"][0]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let suite = match place::suite_file(&manifest, &suite_id) {
        Ok(file) => json!({"suite": suite_id, "file": file, "drafted": false}),
        Err(_) => author::suite_of(harness, &id, &suite_id, cases, rounds)?,
    };
    print_json(&json!({
        "brief": brief_file,
        "product": id,
        "creation": {"state": created["state"], "repositories": created["repositories"], "checkouts": created["checkouts"]},
        "manifest": manifest.file,
        "catalog": {"status": record["status"], "rivals": record["rivals"], "benchmark": record["benchmark"]},
        "suite": suite,
        "next": format!("probierz benchmark author {id} --contender <id> --ours --suite {suite_id}, then one per rival, then probierz benchmark run {id} --suite {suite_id}"),
    }))
}
