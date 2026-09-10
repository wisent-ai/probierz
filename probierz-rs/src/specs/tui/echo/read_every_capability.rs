use crate::specs::{self, tui::common};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
pub fn run(context: &specs::Context) -> Result<(), String> {
    let text = common::required(
        context,
        "ECHO_TEST_ENV",
        "ECHO_TEST_ENV is required; see the echo manifest prerequisites",
    )?;
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return Err("ECHO_TEST_ENV must be an absolute path".into());
    }
    if !path.is_file() {
        return Err("ECHO_TEST_ENV must identify a readable file".into());
    }
    let env = super::common::env_file(&path)?;
    let env = env
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
    for name in ["NEXT_PUBLIC_SUPABASE_URL", "SUPABASE_SERVICE_ROLE_KEY"] {
        if !env.get(name).is_some_and(|v| !v.is_empty()) {
            return Err(format!("{name} is required in ECHO_TEST_ENV"));
        }
    }
    let repo = Path::new("/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/echo");
    let result = common::run(
        "npm",
        &common::strings(&["run", "test:capabilities"]),
        Some(repo),
        &env,
        &[],
        None,
        Duration::from_secs(180),
    )?;
    fs::create_dir_all(&context.artifacts).map_err(|e| e.to_string())?;
    fs::write(
        context.artifacts.join("echo-read-every-capability.tap"),
        format!("{}\n{}", result.stdout, result.stderr),
    )
    .map_err(|e| e.to_string())?;
    if !result.status.success() {
        let all = result.combined();
        return Err(if all.is_empty() {
            format!("Echo capability tests exited {:?}", result.code())
        } else {
            all
        });
    }
    common::write_json(
        &context.artifacts.join("echo-read-every-capability.json"),
        &json!({"command":"npm run test:capabilities","repository":repo,"assertions":["analytics","experiments","onboarding","blogs","generation","assets","products","personas","characters","social","outreach","paid ads","market","reliability","operations"]}),
    )
}
