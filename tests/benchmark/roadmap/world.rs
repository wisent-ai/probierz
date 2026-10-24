//! The world one roadmap journey runs in: a harness, a Stado workspace and a
//! home of its own under Cargo's target directory, a catalog of the one
//! fictional product `alpha`, a suite of one case, and the real probierz and
//! Stado run inside it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");
pub(crate) const APP: &str = "alpha";
pub(crate) const SUITE: &str = "answers";
pub(crate) const CASE: &str = "greeting";
/// The roadmap item Probierz writes for a lost case of `SUITE`.
pub(crate) const ITEM: &str = "Win benchmark case answers/greeting";
/// Probierz's suite and contender-result schemas (`benchmark::SUITE_SCHEMA`,
/// `benchmark::RESULT_SCHEMA`), the protocol the contenders below answer in.
const SUITE_SCHEMA: &str = "ai.wisent.probierz.benchmark.suite.v1";
const RESULT_SCHEMA: &str = "ai.wisent.probierz.benchmark.result.v1";
/// Where Stado reads its catalog under a workspace (`stado-rs/product`'s
/// `CATALOG`).
pub(crate) const CATALOG: &str = "stado/catalog/products.yml";
/// A manifest this repository ships, whose `schemaVersion` is the one the
/// fixture manifest states.
const SHIPPED_MANIFEST: &str = "../apps/probierz-desktop/probierz.yaml";

pub(crate) struct World {
    pub(crate) harness: PathBuf,
    pub(crate) workspace: PathBuf,
    home: PathBuf,
    stado: PathBuf,
}

/// The Stado executable under test; the journey needs the real product, so
/// its absence fails the test naming what to set.
fn stado() -> PathBuf {
    let Some(path) = std::env::var_os("STADO_BIN") else {
        panic!("STADO_BIN is required: the Stado executable under test");
    };
    let path = PathBuf::from(path);
    assert!(path.is_file(), "STADO_BIN {} is not a file", path.display());
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("stado"),
        "STADO_BIN {} must be named stado: Probierz runs `stado` from PATH",
        path.display()
    );
    path
}

/// A fresh harness, workspace and home for one test, with a catalog of the
/// one product `alpha` whose roadmap is empty.
pub(crate) fn world(name: &str) -> World {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("benchmark-roadmap")
        .join(name);
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove the previous run's world");
    }
    let world = World {
        harness: root.join("harness"),
        workspace: root.join("workspace"),
        home: root.join("home"),
        stado: stado(),
    };
    std::fs::create_dir_all(world.harness.join("apps").join(APP)).expect("create the harness");
    std::fs::create_dir_all(&world.home).expect("create the home");
    let catalog = world.workspace.join(CATALOG);
    std::fs::create_dir_all(catalog.parent().expect("the catalog has a directory"))
        .expect("create the catalog directory");
    let schema = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../stado")
            .join(CATALOG),
    )
    .expect("read Stado's own catalog for its schema version");
    let version = schema
        .lines()
        .find_map(|line| line.strip_prefix("schema_version:"))
        .expect("Stado's catalog states its schema_version")
        .trim()
        .to_string();
    std::fs::write(
        &catalog,
        format!(
            "schema_version: {version}\nauthority:\n  owner: stado\n  rule: test catalog\nproducts:\n\
             - id: {APP}\n  name: {APP}\n  status: preview\n  visibility: private\n  family: wisent\n\
             \x20 owner_repository: example-org/{APP}\n  description: The alpha test product.\n\
             \x20 roadmap: []\n  integrations: []\n  evidence:\n  - example-org/{APP}/README.md\n\
             \x20 surfaces:\n  - kind: cli\n    repository: example-org/{APP}\n"
        ),
    )
    .expect("write the test catalog");
    let suite = json!({
        "schema": SUITE_SCHEMA,
        "id": SUITE,
        "version": "first",
        "cases": [{
            "id": CASE,
            "instruction": "Answer with the word right.",
            "assertions": [{"pointer": "/answer", "equals": "right"}],
        }],
    });
    std::fs::write(
        world.harness.join("suite.json"),
        serde_json::to_string_pretty(&suite).expect("render the suite"),
    )
    .expect("write the suite");
    world
}

/// The manifest schema version a shipped manifest states.
fn manifest_schema() -> serde_yaml::Value {
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join(SHIPPED_MANIFEST);
    let text = match std::fs::read_to_string(&shipped) {
        Ok(text) => text,
        Err(error) => panic!("read {}: {error}", shipped.display()),
    };
    let document: serde_yaml::Value = match serde_yaml::from_str(&text) {
        Ok(document) => document,
        Err(error) => panic!("{} is not YAML: {error}", shipped.display()),
    };
    document["schemaVersion"].clone()
}

/// A contender that reads its task and answers `answer` under `/answer`.
fn contender(answer: &str, ours: bool) -> Value {
    let result = json!({"schema": RESULT_SCHEMA, "status": "completed", "output": {"answer": answer}});
    json!({
        "program": "/bin/sh",
        "args": ["-c", format!("while read -r line; do :; done; printf '%s' '{result}'")],
        "ours": ours,
    })
}

/// The product's manifest, with our contender answering `ours_answers`.
pub(crate) fn manifest(world: &World, ours_answers: &str) {
    let mut document: serde_yaml::Value = serde_yaml::to_value(json!({
        "appId": APP,
        "productId": APP,
        "owner": "alpha maintainers",
        "repositories": [{"root": world.harness.to_string_lossy(), "mappings": []}],
        "surfaces": {},
        "journeys": {},
        "benchmark": {
            "suites": {SUITE: world.harness.join("suite.json").to_string_lossy()},
            "contenders": {
                "ours": contender(ours_answers, true),
                "rival": contender("right", false),
            },
        },
    }))
    .expect("the manifest is YAML");
    document["schemaVersion"] = manifest_schema();
    std::fs::write(
        world.harness.join("apps").join(APP).join("probierz.yaml"),
        serde_yaml::to_string(&document).expect("render the manifest"),
    )
    .expect("write the manifest");
}

/// The real Stado first on PATH, in the test's own workspace and home.
fn environment(command: &mut Command, world: &World) {
    let directory = world.stado.parent().expect("STADO_BIN has a directory");
    let path = format!(
        "{}:{}",
        directory.display(),
        std::env::var("PATH").expect("PATH is set")
    );
    command
        .env("PATH", path)
        .env("HOME", &world.home)
        .env("WISENT_WORKSPACE", &world.workspace)
        .env("PROBIERZ_HARNESS_DIR", &world.harness);
}

pub(crate) fn probierz(world: &World, args: &[&str]) -> Output {
    let mut command = Command::new(PROBIERZ);
    command.args(args);
    environment(&mut command, world);
    command.output().expect("start probierz")
}

pub(crate) fn answered(output: &Output, step: &str) -> Value {
    assert!(
        output.status.success(),
        "{step} failed: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    match serde_json::from_slice(&output.stdout) {
        Ok(answer) => answer,
        Err(error) => panic!("{step} answered no JSON ({error}): {output:?}"),
    }
}

/// The real Stado run with `args` in the test's own workspace and home.
pub(crate) fn stado_run(world: &World, args: &[&str]) -> Output {
    let mut command = Command::new(&world.stado);
    command.args(args);
    environment(&mut command, world);
    command.output().expect("start stado")
}

/// Product `alpha`'s roadmap as the real Stado serves it.
pub(crate) fn roadmap(world: &World) -> Vec<Value> {
    let catalog = answered(
        &stado_run(world, &["product", "catalog", "--json"]),
        "stado product catalog",
    );
    let Some(product) = catalog["products"]
        .as_array()
        .and_then(|products| products.iter().find(|product| product["id"] == APP))
    else {
        panic!("the catalog holds no product {APP}: {catalog}");
    };
    let Some(items) = product["roadmap"].as_array() else {
        panic!("{APP}'s roadmap is not a list: {product}");
    };
    items.clone()
}

/// The one item of `roadmap` titled `title`; none or several fail the test.
pub(crate) fn the_item(roadmap: &[Value], title: &str) -> Value {
    let titled: Vec<&Value> = roadmap.iter().filter(|item| item["title"] == title).collect();
    let [item] = titled.as_slice() else {
        panic!("the roadmap should hold exactly one item {title}: {roadmap:?}");
    };
    (*item).clone()
}
