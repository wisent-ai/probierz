use crate::specs::{self, tui::common};
use serde_json::json;
use std::{collections::BTreeMap, time::Duration};
pub fn run(context: &specs::Context) -> Result<(), String> {
    let repo = specs::wisent_checkout("echo-landing");
    let repo = repo.as_path();
    let (result, logs) = super::common::server_test(
        repo,
        "/docs",
        "test:docs",
        BTreeMap::new(),
        None,
        "Echo docs",
        Duration::from_secs(300),
    )?;
    if !result.status.success() {
        let all = format!("{}{}{}", result.stdout, result.stderr, logs);
        return Err(if all.is_empty() {
            format!("Echo docs tests exited {:?}", result.code())
        } else {
            all
        });
    }
    common::write_json(
        &context.artifacts.join("echo-docs-capabilities.json"),
        &json!({"command":"npm run test:docs","repository":repo,"assertions":["capability catalogue","17 canonical CLI command pages","CLI index links","traffic analytics decision guide"]}),
    )
}
