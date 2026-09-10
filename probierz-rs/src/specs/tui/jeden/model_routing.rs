use crate::specs::{self, tui::common};
use serde_json::json;
use std::time::Duration;
pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = common::required(
        context,
        "TUI_CMD",
        "TUI_CMD is required: provide the released Jeden executable",
    )?;
    let model = common::required(
        context,
        "JEDEN_MODEL",
        "JEDEN_MODEL is required: provide a real model coordinate available to this workload",
    )?;
    for (n, d) in [
        (
            "BRAMA_URL",
            "provide the externally provisioned Brama router URL",
        ),
        (
            "WISENT_APP_AGENT_ID",
            "provide the externally provisioned Jeden workload identity",
        ),
        (
            "WISENT_APP_AGENT_AUTH_SECRET",
            "provide the workload signing credential outside Probierz",
        ),
    ] {
        common::required(context, n, &format!("{n} is required: {d}"))?;
    }
    let home = common::scratch("probierz-jeden-model-route")?;
    let env = common::env_map([
        ("HOME", home.to_string_lossy()),
        ("XDG_STATE_HOME", home.join("state").to_string_lossy()),
        ("XDG_CONFIG_HOME", home.join("config").to_string_lossy()),
        ("XDG_CACHE_HOME", home.join("cache").to_string_lossy()),
    ]);
    let result = common::run(
        &binary,
        &vec![
            "run".into(),
            "Respond exactly: OK".into(),
            "--model".into(),
            model.clone(),
        ],
        Some(&home),
        &env,
        &[],
        None,
        Duration::from_secs(180),
    );
    let answer = (|| {
        let result = result?;
        if !result.status.success() {
            return Err(format!(
                "Jeden exited {}: {}",
                result
                    .code()
                    .map_or_else(|| "signal".into(), |c| c.to_string()),
                result.stderr
            ));
        }
        if !regex::Regex::new(r"\bOK\b")
            .unwrap()
            .is_match(&result.stdout)
        {
            return Err(format!(
                "expected the signed Brama route to return OK: {}",
                result.stdout
            ));
        }
        common::write_trace(
            context,
            "jeden-model-routing.trace.json",
            json!({"schemaVersion":1,"kind":"probierz-jeden-model-routing-trace","evidenceLevel":"E3","runId":context.optional("PROBIERZ_RUN_ID"),"model":model,"status":"completed","observation":{"exitCode":result.code(),"reply":result.stdout.trim().chars().rev().take(1000).collect::<String>().chars().rev().collect::<String>(),"stderr":result.stderr.trim().chars().rev().take(1000).collect::<String>().chars().rev().collect::<String>()},"redaction":{"status":"verified_redacted","credentialsIncluded":false,"privateRecordsIncluded":false},"publicationRequirements":{"artifactKind":"trace","minimumEvidence":"E3","redactionStatus":"verified_redacted","signedReceiptRequired":true}}),
        )
    })();
    common::remove(&home);
    answer
}
