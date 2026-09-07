use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};

use serde_json::Value;

use crate::specs::{self, tui::common};

const PRODUCT_SPECS: [(&str, &str, u64); 10] = [
    (
        "host-dynamic-capacity",
        "tests/host_dynamic_capacity/probierz.spec.mjs",
        5_100,
    ),
    (
        "run-retention",
        "tests/run_history/probierz.spec.mjs",
        5_100,
    ),
    ("disk-cleanup", "tests/cleanup/probierz.spec.mjs", 5_100),
    ("release-pipeline", "tests/ci-cd/probierz.spec.mjs", 8_100),
    ("native-build", "tests/builds/probierz.spec.mjs", 5_700),
    (
        "platform-matrix",
        "tests/platform-matrix/probierz.spec.mjs",
        7_200,
    ),
    (
        "native-readers",
        "tests/native_readers/probierz.spec.mjs",
        5_400,
    ),
    (
        "service-convergence",
        "tests/service_convergence/probierz.spec.mjs",
        5_700,
    ),
    (
        "apple-challenge-preparation",
        "tests/apple_challenge/preparation.probierz.spec.mjs",
        900,
    ),
    (
        "apple-challenge-readiness",
        "tests/apple_challenge/readiness.probierz.spec.mjs",
        900,
    ),
];

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = context
        .optional("TUI_CMD")
        .ok_or_else(|| "TUI_CMD must identify the staged Stado binary".to_string())?;
    let crate_root = match context.optional("PROBIERZ_APP_SOURCE") {
        Some(source) => resolve_from(&context.harness, &source).join("stado-rs"),
        None => {
            let binary = resolve_from(&context.harness, &binary);
            binary.parent().unwrap_or(&binary).join("../..")
        }
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

    let mut base_env = BTreeMap::from([
        ("TUI_CMD".to_string(), binary),
        (
            "PROBIERZ_ARTIFACTS".to_string(),
            context.artifacts.to_string_lossy().into_owned(),
        ),
    ]);
    for name in [
        "PROBIERZ_APP_SOURCE",
        "PROBIERZ_JOURNEYS",
        "PROBIERZ_PLATFORM",
        "PROBIERZ_RUN_ID",
    ] {
        if let Some(value) = context.optional(name) {
            base_env.insert(name.to_string(), value);
        }
    }

    for journey in journeys {
        let Some((_, relative, timeout_seconds)) =
            PRODUCT_SPECS.iter().find(|(known, _, _)| *known == journey)
        else {
            return Err(format!(
                "Unmapped Stado journey selected by Probierz: {journey}"
            ));
        };
        let media_manifest = context
            .artifacts
            .join(format!(".stado-journeys-{journey}-media.json"));
        let mut env = base_env.clone();
        env.insert(
            "PROBIERZ_MEDIA_MANIFEST".into(),
            media_manifest.to_string_lossy().into_owned(),
        );
        let spec = crate_root.join(relative);
        let output = common::run(
            "node",
            &[spec.to_string_lossy().into_owned()],
            Some(&context.harness),
            &env,
            &[],
            None,
            Duration::from_secs(*timeout_seconds),
        )?;
        let media_result = record_media(context, &media_manifest);
        let _ = fs::remove_file(&media_manifest);
        media_result?;
        if !output.status.success() {
            return Err(format!(
                "Stado journey {journey} exited {}\n{}",
                output
                    .code()
                    .map_or_else(|| "signal".into(), |code| code.to_string()),
                output.combined()
            ));
        }
    }
    Ok(())
}

fn record_media(context: &specs::Context, path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let entries: Vec<Value> = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not JSON: {error}", path.display()))?;
    for entry in entries {
        let file = entry
            .get("file")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{} contains media without a file", path.display()))?;
        let kind = match entry.get("kind").and_then(Value::as_str) {
            Some("screenshot") => "screenshot",
            Some("trace") => "trace",
            Some("video") => "video",
            Some(kind) => return Err(format!("unsupported media kind {kind}")),
            None => return Err(format!("{} contains media without a kind", path.display())),
        };
        if let Some(content_type) = entry.get("contentType").and_then(Value::as_str) {
            context.media_typed(kind, file, content_type);
        } else {
            context.media(kind, file);
        }
    }
    Ok(())
}

fn resolve_from(base: &std::path::Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}
