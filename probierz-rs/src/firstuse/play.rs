//! One first-use run, played as a new user on a clean home.
//!
//! The run gets a home of its own under the harness's `test-results`, an
//! environment holding nothing of the operator's but `PATH` (so the user's
//! tools, not their credentials or settings, are on hand), and the product's
//! public install instructions. It then walks the definition's path, runs the
//! command each screen shows with the user's own data in place of its
//! placeholders, and reads the first-success fact where the product says it
//! records the journey. Each step is kept with its argv, exit status and both
//! output streams, under the event names of the product's own analytics
//! contract, so the run is the journey's evidence and its progress in one
//! record.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value as Json};

use super::declare::Declared;
use super::walk;
use crate::failure::{ended, now_iso, Failure};

/// The directories a clean home starts with, and the variables naming them.
const HOME_DIRECTORIES: &[(&str, &str)] = &[
    ("XDG_CONFIG_HOME", ".config"),
    ("XDG_DATA_HOME", ".local/share"),
    ("XDG_STATE_HOME", ".local/state"),
    ("XDG_CACHE_HOME", ".cache"),
];

pub(crate) struct Sandbox {
    pub root: PathBuf,
    pub home: PathBuf,
    path: std::ffi::OsString,
}

impl Sandbox {
    pub fn create(root: PathBuf, declared: &Declared) -> Result<Self, Failure> {
        let home = root.join("home");
        for (_, directory) in HOME_DIRECTORIES {
            std::fs::create_dir_all(home.join(directory))?;
        }
        std::fs::create_dir_all(root.join("tmp"))?;
        std::fs::create_dir_all(root.join("output"))?;
        let inherited = std::env::var_os("PATH").ok_or_else(|| {
            Failure::invalid(
                "first-use.path",
                "PATH is not set, so a new user's tools cannot be found",
            )
        })?;
        let mut entries: Vec<PathBuf> =
            declared.path.iter().map(|entry| home.join(entry)).collect();
        entries.extend(std::env::split_paths(&inherited));
        let path = std::env::join_paths(entries).map_err(|error| {
            Failure::invalid(
                "first-use.path",
                format!("the run's PATH cannot be formed: {error}"),
            )
        })?;
        Ok(Self { root, home, path })
    }

    /// Run `argv` as the new user, keeping both streams under `name`.
    pub fn run(&self, name: &str, argv: &[String]) -> Result<Json, Failure> {
        let (program, arguments) = argv
            .split_first()
            .ok_or_else(|| Failure::config("first-use.step", format!("{name} names no program")))?;
        let output_directory = self.root.join("output");
        let mut command = Command::new(program);
        command
            .args(arguments)
            .current_dir(&self.home)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("TMPDIR", self.root.join("tmp"));
        for (variable, directory) in HOME_DIRECTORIES {
            command.env(variable, self.home.join(directory));
        }
        let output = crate::wait::output(&mut command).map_err(|error| {
            Failure::new(
                "first-use.step",
                crate::failure::Code::Prerequisite,
                format!("{name}: {} could not start: {error}", argv.join(" ")),
            )
        })?;
        let stdout = output_directory.join(format!("{name}.stdout"));
        let stderr = output_directory.join(format!("{name}.stderr"));
        std::fs::write(&stdout, &output.stdout)?;
        std::fs::write(&stderr, &output.stderr)?;
        Ok(json!({
            "argv": argv,
            "succeeded": output.status.success(),
            "exit": ended(&output.status),
            "stdout": stdout.to_string_lossy(),
            "stderr": stderr.to_string_lossy(),
        }))
    }
}

/// The value the product recorded at `pointer` in its progress file, or why
/// it could not be read.
fn recorded(home: &Path, file: &str, pointer: &str) -> Json {
    let location = home.join(file);
    match std::fs::read(&location) {
        Err(error) => {
            json!({ "file": location.to_string_lossy(), "unread": format!("cannot be read: {error}") })
        }
        Ok(bytes) => match serde_json::from_slice::<Json>(&bytes) {
            Err(error) => {
                json!({ "file": location.to_string_lossy(), "unread": format!("is not JSON: {error}") })
            }
            Ok(document) => {
                json!({ "file": location.to_string_lossy(), "observed": document.pointer(pointer) })
            }
        },
    }
}

/// Play the journey in `sandbox` and answer its record; `passed` is true only
/// when every install step and every shown command succeeded and the product
/// recorded the first-success fact with the value the definition expects.
pub(crate) fn play(declared: &Declared, sandbox: &Sandbox) -> Result<Json, Failure> {
    let definition = &declared.definition;
    let screens = walk::path(definition)?;
    let (fact, expected) = walk::expected_first_success(definition)?;
    let contract = &definition["analytics_contract"];
    let started_at = now_iso();

    let mut install = Vec::new();
    for (index, argv) in declared.install.iter().enumerate() {
        install.push(sandbox.run(&format!("install-{index}"), argv)?);
    }
    let installed = install.iter().all(|step| step["succeeded"] == true);

    let mut steps = Vec::new();
    if installed {
        for screen in &screens {
            let screen_id = screen["screen_id"].as_str().ok_or_else(|| {
                Failure::config(
                    "first-use.definition",
                    "a screen on the path carries no screen_id",
                )
            })?;
            let mut step = json!({
                "screenId": screen_id,
                "screenKind": screen["screen_kind"],
                "required": screen["required"],
                "events": [contract["exposure_event"]],
            });
            if let Some(argv) = walk::argv(screen, &declared.fixtures)? {
                let ran = sandbox.run(&format!("screen-{screen_id}"), &argv)?;
                let succeeded = ran["succeeded"] == true;
                step["command"] = ran;
                if !succeeded {
                    steps.push(step);
                    break;
                }
            }
            step["events"] = json!([contract["exposure_event"], contract["primary_action_event"]]);
            steps.push(step);
        }
    }
    let walked = installed
        && steps.len() == screens.len()
        && steps
            .iter()
            .all(|step| step["command"]["succeeded"] != false);
    let observed = recorded(
        &sandbox.home,
        &declared.progress.file,
        &declared.progress.fact,
    );
    let held = walked && observed["observed"] == expected;
    let mut events: Vec<Json> = Vec::new();
    if held {
        events.push(contract["first_success_event"].clone());
        events.push(contract["completion_event"].clone());
    }
    Ok(json!({
        "startedAt": started_at,
        "finishedAt": now_iso(),
        "home": sandbox.home.to_string_lossy(),
        "install": install,
        "installed": installed,
        "steps": steps,
        "walked": walked,
        "firstSuccess": {
            "fact": fact,
            "expected": expected,
            "pointer": declared.progress.fact,
            "record": observed,
            "held": held,
        },
        "events": events,
        "passed": held,
    }))
}
