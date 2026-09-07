use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};

use crate::specs::{self, tui::common};

const PRODUCT_SPECS: [(&str, &str, u64); 2] = [
    (
        "cli-scoped-generation",
        "tests/docs/cli-scoped-generation.probierz.spec.mjs",
        180,
    ),
    (
        "cli-command-pages",
        "tests/docs/cli-commands.probierz.spec.mjs",
        600,
    ),
];

pub fn run(context: &specs::Context) -> Result<(), String> {
    let source = match context.optional("PROBIERZ_APP_SOURCE") {
        Some(source) => resolve_from(&context.harness, &source),
        None => manifest_source(context)?,
    };
    let journeys = context
        .optional("PROBIERZ_JOURNEYS")
        .unwrap_or_default()
        .split(',')
        .filter(|journey| !journey.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if journeys.is_empty() {
        return Err("PROBIERZ_JOURNEYS must name the journeys selected by Probierz".into());
    }

    let mut env = BTreeMap::from([(
        "PROBIERZ_ARTIFACTS".to_string(),
        context.artifacts.to_string_lossy().into_owned(),
    )]);
    for name in ["PROBIERZ_RUN_ID", "PROBIERZ_JOURNEYS", "STADO_BIN"] {
        if let Some(value) = context.optional(name) {
            env.insert(name.to_string(), value);
        }
    }

    for journey in journeys {
        let Some((_, relative, timeout_seconds)) =
            PRODUCT_SPECS.iter().find(|(known, _, _)| *known == journey)
        else {
            return Err(format!(
                "Unmapped Stado documentation journey selected by Probierz: {journey}"
            ));
        };
        let spec = source.join(relative);
        let result = common::run(
            "node",
            &[spec.to_string_lossy().into_owned()],
            Some(&context.harness),
            &env,
            &[],
            None,
            Duration::from_secs(*timeout_seconds),
        )?;
        if !result.status.success() {
            return Err(format!(
                "Stado documentation journey {journey} exited {}\n{}",
                result
                    .code()
                    .map_or_else(|| "signal".into(), |code| code.to_string()),
                result.combined()
            ));
        }
    }
    Ok(())
}

fn manifest_source(context: &specs::Context) -> Result<PathBuf, String> {
    let manifest_path = context.harness.join("apps/stado-docs/probierz.yaml");
    let text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    let manifest: serde_yaml::Value = serde_yaml::from_str(&text)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    let root = manifest
        .get("repositories")
        .and_then(serde_yaml::Value::as_sequence)
        .and_then(|repositories| repositories.first())
        .and_then(|repository| repository.get("root"))
        .and_then(serde_yaml::Value::as_str)
        .ok_or_else(|| {
            "the stado-docs manifest must provide the source repository root".to_string()
        })?;
    Ok(resolve_from(&context.harness, root))
}

fn resolve_from(base: &std::path::Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}
