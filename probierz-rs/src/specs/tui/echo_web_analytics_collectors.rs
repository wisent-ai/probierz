use crate::specs::{self, tui::common};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
pub fn run(context: &specs::Context) -> Result<(), String> {
    let text = common::required(
        context,
        "ECHO_ANALYTICS_TEST_ENV",
        "ECHO_ANALYTICS_TEST_ENV is required; see the echo-web manifest prerequisites",
    )?;
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return Err("ECHO_ANALYTICS_TEST_ENV must be an absolute path".into());
    }
    if !path.is_file() {
        return Err("ECHO_ANALYTICS_TEST_ENV must identify a readable file".into());
    }
    let mut env = super::echo_common::env_file(&path)?;
    for name in ["NEXT_PUBLIC_SUPABASE_URL", "SUPABASE_SERVICE_ROLE_KEY"] {
        if !env.get(name).is_some_and(|v| !v.is_empty()) {
            return Err(format!("{name} is required in ECHO_ANALYTICS_TEST_ENV"));
        }
    }
    env.entry("NEXT_PUBLIC_SITE_URL".into())
        .or_insert_with(|| "https://echo.wisent.com".into());
    let repo = Path::new("/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/echo-web");
    let (result, logs) = super::echo_common::server_test(
        repo,
        "/api/health",
        "test:analytics",
        env,
        Some(".next-probierz-analytics"),
        "Echo",
        Duration::from_secs(120),
    )?;
    if !result.status.success() {
        let all = format!("{}{}{}", result.stdout, result.stderr, logs);
        return Err(if all.is_empty() {
            format!("analytics tests exited {:?}", result.code())
        } else {
            all
        });
    }
    common::write_json(
        &context.artifacts.join("echo-web-analytics-collectors.json"),
        &json!({"command":"npm run test:analytics","repository":repo,"assertions":["web persisted once","mobile persisted once","invalid clients refused"]}),
    )
}
