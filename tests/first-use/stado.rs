//! `probierz run first-use/stado` through the real binary, as a new Stado user
//! meets it.
//!
//! The passing run installs the exact signed release with Stado's own public
//! `install-stado.sh` into a clean home Probierz makes for it, walks the CLI's
//! first-use definition, imports the user's registry export, and only then
//! reads Stado's own onboarding record: `~/.stado/onboarding.json` under that
//! home must hold `evidence.registry_configuration_accepted: true`, and the
//! installed `stado` must be there. The values the declaration's `inputs`
//! name (Stado's quick start asks a new user for the control origin and the
//! exact release) are the user's to state; this test takes them from its own
//! environment and fails naming them when they are not given, because without
//! them nothing was installed.
//!
//! The refusals run against a harness made for the case under the package's
//! own build directory, beside the real Stado manifest: an app whose
//! `first-use.yaml` is absent is refused naming that file, an install that
//! fails is a failed first use whose record says it never installed, and a
//! declared input the caller did not give is refused by its name before
//! anything runs.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value as Json;

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");
const APP: &str = "stado";
const TARGET: &str = "first-use/stado";
const DECLARATION: &str = "first-use.yaml";

/// The harness this binary was built in: `apps/stado/` holds the real
/// manifest and its first-use declaration.
fn real_harness() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the harness")
        .to_path_buf()
}

fn probierz(harness: &Path, configure: impl FnOnce(&mut Command)) -> Output {
    let mut command = Command::new(PROBIERZ);
    command.arg("--harness").arg(harness).args(["run", TARGET]);
    configure(&mut command);
    command.output().expect("start probierz")
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A file the test needs, or a panic naming it and why it could not be read.
fn read(file: &Path) -> Vec<u8> {
    match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) => panic!("{} cannot be read: {error}", file.display()),
    }
}

/// Stado's real first-use declaration, as the harness holds it.
fn real_declaration() -> serde_yaml::Value {
    let file = real_harness().join("apps").join(APP).join(DECLARATION);
    serde_yaml::from_slice(&read(&file)).expect("the declaration is YAML")
}

/// The variable names the declaration asks the new user to state.
fn declared_inputs() -> Vec<String> {
    real_declaration()["inputs"]
        .as_sequence()
        .expect("Stado's first use declares the inputs its installer reads")
        .iter()
        .map(|name| {
            name.as_str()
                .expect("an input is a variable name")
                .to_string()
        })
        .collect()
}

/// A harness of its own for one refusal, holding a copy of Stado's real
/// manifest and, when given, a first-use declaration.
fn case_harness(case: &str, declaration: Option<&serde_yaml::Value>) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("first-use-{case}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove the previous case harness");
    }
    let app = root.join("apps").join(APP);
    std::fs::create_dir_all(&app).expect("create the case app directory");
    std::fs::copy(
        real_harness().join("apps").join(APP).join("probierz.yaml"),
        app.join("probierz.yaml"),
    )
    .expect("copy Stado's manifest");
    if let Some(declaration) = declaration {
        std::fs::write(
            app.join(DECLARATION),
            serde_yaml::to_string(declaration).expect("encode the declaration"),
        )
        .expect("write the declaration");
    }
    root
}

/// Whether any run of the case harness got as far as installing into its home.
/// No runs directory means no run started; one that cannot be listed fails
/// the test with its cause, never reads as nothing installed.
fn anything_installed(harness: &Path) -> bool {
    let runs = harness.join("test-results").join(APP).join("first-use");
    let entries = match runs.read_dir() {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(error) => panic!(
            "the case's runs at {} cannot be listed: {error}",
            runs.display()
        ),
    };
    entries
        .map(|entry| match entry {
            Ok(run) => run.path().join("home").join(".stado").exists(),
            Err(error) => panic!("a run under {} cannot be read: {error}", runs.display()),
        })
        .any(|installed| installed)
}

#[test]
fn a_new_user_installs_stado_and_reaches_its_first_success() {
    let missing: Vec<String> = declared_inputs()
        .into_iter()
        .filter(|name| std::env::var_os(name).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "the first use installs a release the user states: give {} in this test's environment (Stado's quick start asks a new user for them)",
        missing.join(" and ")
    );
    let output = probierz(&real_harness(), |_| {});
    assert!(output.status.success(), "{}", said(&output));
    let record: Json =
        serde_json::from_slice(&output.stdout).expect("the run prints its record as JSON");
    assert_eq!(record["installed"], Json::Bool(true), "{record}");
    assert_eq!(record["walked"], Json::Bool(true), "{record}");
    assert_eq!(record["firstSuccess"]["held"], Json::Bool(true), "{record}");

    let home = PathBuf::from(record["home"].as_str().expect("the record names its home"));
    let installed = home.join(".stado").join("bin").join("stado");
    assert!(installed.is_file(), "no stado at {}", installed.display());
    let onboarding = home.join(".stado").join("onboarding.json");
    let state: Json =
        serde_json::from_slice(&read(&onboarding)).expect("Stado's onboarding record is JSON");
    assert_eq!(
        state.pointer("/evidence/registry_configuration_accepted"),
        Some(&Json::Bool(true)),
        "{state}"
    );
    let kept = home
        .parent()
        .expect("the home sits in the run directory")
        .join("record.json");
    assert!(
        kept.is_file(),
        "the record was not kept at {}",
        kept.display()
    );
}

#[test]
fn an_app_without_a_first_use_declaration_is_refused_naming_the_file() {
    let harness = case_harness("undeclared", None);
    let output = probierz(&harness, |_| {});
    assert!(!output.status.success(), "{}", said(&output));
    let text = said(&output);
    assert!(
        text.contains(DECLARATION) && text.contains("declares no first use"),
        "{text}"
    );
    assert!(
        !harness.join("test-results").exists(),
        "a refused run left results under {}",
        harness.display()
    );
    std::fs::remove_dir_all(&harness).expect("remove the case harness");
}

#[test]
fn a_failed_install_is_a_failed_first_use_with_its_record() {
    let mut declaration = real_declaration();
    declaration["install"] = serde_yaml::from_str("[[\"false\"]]").expect("an install that fails");
    declaration["inputs"] = serde_yaml::Value::Sequence(Vec::new());
    let harness = case_harness("failed-install", Some(&declaration));
    let output = probierz(&harness, |_| {});
    assert!(!output.status.success(), "{}", said(&output));
    let record: Json =
        serde_json::from_slice(&output.stdout).expect("a failed first use still prints its record");
    assert_eq!(record["installed"], Json::Bool(false), "{record}");
    assert_eq!(record["passed"], Json::Bool(false), "{record}");
    assert!(
        record["steps"].as_array().is_some_and(Vec::is_empty),
        "no screen may be walked after a failed install: {record}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("did not reach its first-success fact"),
        "{}",
        said(&output)
    );
    std::fs::remove_dir_all(&harness).expect("remove the case harness");
}

#[test]
fn an_input_the_user_did_not_state_is_refused_by_name() {
    let inputs = declared_inputs();
    let (withheld, stated) = inputs
        .split_last()
        .expect("Stado's first use declares at least one input");
    let harness = case_harness("missing-input", Some(&real_declaration()));
    let output = probierz(&harness, |command| {
        for name in stated {
            command.env(name, "stated-by-the-test");
        }
        command.env_remove(withheld);
    });
    assert!(!output.status.success(), "{}", said(&output));
    let text = said(&output);
    assert!(text.contains(&format!("{withheld} is not set")), "{text}");
    assert!(
        !anything_installed(&harness),
        "nothing may be installed for a refused run"
    );
    std::fs::remove_dir_all(&harness).expect("remove the case harness");
}
