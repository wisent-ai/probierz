use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};

use serde_json::Value;

use crate::{
    failure::write_private,
    specs::{self, tui::common},
};

const CANONICAL_REPOSITORY: &str =
    "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/wisent-backend";

pub fn run(context: &specs::Context) -> Result<(), String> {
    let canonical = PathBuf::from(CANONICAL_REPOSITORY);
    let repository = if canonical.exists() {
        canonical
    } else {
        std::env::current_dir()
            .map_err(|error| format!("cannot resolve the current directory: {error}"))?
            .join("../wisent-backend")
    };
    let script = repository.join("tests/chat/production_latency.py");

    let credential = common::run(
        "skarbiec",
        &common::strings(&["get", "wisent-backend-supabase"]),
        None,
        &BTreeMap::new(),
        &[],
        None,
        Duration::from_secs(120),
    )?;
    if !credential.status.success() {
        return Err(if credential.stderr.is_empty() {
            "Skarbiec could not read wisent-backend-supabase".into()
        } else {
            credential.stderr
        });
    }
    let credential: Value = serde_json::from_str(&credential.stdout)
        .map_err(|error| format!("Skarbiec returned invalid JSON: {error}"))?;
    let fields = credential
        .get("fields")
        .and_then(Value::as_object)
        .ok_or_else(|| "Skarbiec returned no fields for wisent-backend-supabase".to_string())?;
    let url = fields
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "wisent-backend-supabase.url is required".to_string())?;
    let anon_key = fields
        .get("anon_key")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "wisent-backend-supabase.anon_key is required".to_string())?;

    let mut env = BTreeMap::from([
        ("SUPABASE_URL".to_string(), url.to_string()),
        ("SUPABASE_ANON_KEY".to_string(), anon_key.to_string()),
        (
            "TEST_EMAIL".to_string(),
            context
                .optional("TEST_EMAIL")
                .unwrap_or_else(|| "wisent+backend_hardcoded@wisent.ai".into()),
        ),
        (
            "TEST_PASSWORD".to_string(),
            context
                .optional("TEST_PASSWORD")
                .unwrap_or_else(|| "123456".into()),
        ),
    ]);
    for name in [
        "CHAT_CHARACTER_ID",
        "CHAT_ENDPOINT",
        "CHAT_SITE_ORIGIN",
        "CHAT_TIMEOUT_SECONDS",
        "CHAT_USER_AGENT",
        "EXISTING_CONVERSATION_SAMPLES",
        "NEW_CONVERSATION_SAMPLES",
        "SOURCE_REVISION",
        "SUPABASE_KEY",
    ] {
        if let Some(value) = context.optional(name) {
            env.insert(name.to_string(), value);
        }
    }
    let result = common::run(
        "python3",
        &[script.to_string_lossy().into_owned()],
        Some(&repository),
        &env,
        &[],
        None,
        Duration::from_secs(1_200),
    )?;

    fs::create_dir_all(&context.artifacts)
        .map_err(|error| format!("{}: {error}", context.artifacts.display()))?;
    let artifact = context
        .artifacts
        .join("wisent-backend-production-latency.json");
    let evidence = if result.stdout.is_empty() {
        result.stderr.as_bytes()
    } else {
        result.stdout.as_bytes()
    };
    write_private(&artifact, evidence)
        .map_err(|error| format!("{}: {error}", artifact.display()))?;
    context.media_typed("trace", artifact, "application/json");

    if !result.status.success() {
        let output = [&result.stdout, &result.stderr]
            .into_iter()
            .filter(|value| !value.is_empty())
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        return Err(if output.is_empty() {
            format!(
                "Production latency measurement exited {}",
                result
                    .code()
                    .map_or_else(|| "signal".into(), |code| code.to_string())
            )
        } else {
            output
        });
    }
    Ok(())
}
