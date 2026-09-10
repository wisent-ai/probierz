use crate::specs::{self, tui::common};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
pub fn run(context: &specs::Context) -> Result<(), String> {
    let text = common::required(
        context,
        "ECHO_GUI_TEST_ENV",
        "ECHO_GUI_TEST_ENV is required; see the echo-web-gui manifest prerequisites",
    )?;
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return Err("ECHO_GUI_TEST_ENV must be an absolute path".into());
    }
    if !path.is_file() {
        return Err("ECHO_GUI_TEST_ENV must identify a readable file".into());
    }
    let env = super::common::env_file(&path)?;
    let mut env = env
        .into_iter()
        .filter_map(|(name, value)| {
            let name = name.trim_start().to_string();
            let renamed = name.strip_prefix("export").and_then(|rest| {
                rest.chars()
                    .next()
                    .is_some_and(char::is_whitespace)
                    .then(|| rest.trim_start().to_string())
            });
            let name = renamed.unwrap_or(name);
            (!name.is_empty()).then_some((name, value))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for value in env.values_mut() {
        if value.ends_with("\\n") {
            value.truncate(value.len() - 2);
        }
    }
    for name in [
        "NEXT_PUBLIC_SUPABASE_URL",
        "NEXT_PUBLIC_SUPABASE_ANON_KEY",
        "SUPABASE_SERVICE_ROLE_KEY",
    ] {
        if !env.get(name).is_some_and(|v| !v.is_empty()) {
            return Err(format!("{name} is required in ECHO_GUI_TEST_ENV"));
        }
    }
    if env.get("NEXT_PUBLIC_SITE_URL").is_none_or(String::is_empty) {
        env.insert(
            "NEXT_PUBLIC_SITE_URL".into(),
            "https://echo.wisent.com".into(),
        );
    }
    let repo = Path::new("/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/echo-web");
    let (result, logs) = super::common::server_test(
        repo,
        "/api/health",
        "test:capability-gui",
        env,
        Some(".next-probierz-capabilities"),
        "Echo",
        Duration::from_secs(120),
    )?;
    if !result.status.success() {
        let all = format!("{}{}{}", result.stdout, result.stderr, logs);
        return Err(if all.is_empty() {
            format!("Echo GUI test exited {:?}", result.code())
        } else {
            all
        });
    }
    common::write_json(
        &context.artifacts.join("echo-web-capability-catalogue.json"),
        &json!({"command":"npm run test:capability-gui","repository":repo,"assertions":["real approved admin","protected GUI","11 workspaces","documentation link","CLI commands"]}),
    )
}
