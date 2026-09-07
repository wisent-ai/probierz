use crate::specs::{self, tui::common};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, time::Duration};
fn invoke(
    binary: &str,
    args: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    db: &Path,
    assets: &Path,
) -> Result<Value, String> {
    let mut argv = vec![
        "--db".into(),
        db.to_string_lossy().into_owned(),
        "--asset-dir".into(),
        assets.to_string_lossy().into_owned(),
        "--actor".into(),
        "probierz-first-use".into(),
    ];
    argv.extend_from_slice(args);
    let out = common::run(
        binary,
        &argv,
        Some(cwd),
        env,
        &[
            "STADO_INTEGRATION_API_URL",
            "UGC_CLI_STADO_INTEGRATION_TOKEN",
        ],
        None,
        Duration::from_secs(120),
    )?;
    if !out.status.success() {
        return Err(format!(
            "ugc exited {:?}\nstdout:\n{}\nstderr:\n{}",
            out.code(),
            out.stdout,
            out.stderr
        ));
    }
    common::parse_json(&out.stdout, "ugc")
}
fn one<'a>(v: &'a Value, key: &str, label: &str) -> Result<&'a Value, String> {
    let values = v[key]
        .as_object()
        .ok_or_else(|| format!("isolated {label} must contain exactly one record"))?;
    if values.len() != 1 {
        return Err(format!("isolated {label} must contain exactly one record"));
    }
    Ok(values.values().next().unwrap())
}
pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = common::required(
        context,
        "PROBIERZ_UGC_CLI_BINARY",
        "PROBIERZ_UGC_CLI_BINARY is required; Probierz will not invent a UGC CLI build path",
    )?;
    if !Path::new(&binary).is_absolute() {
        return Err(
            "PROBIERZ_UGC_CLI_BINARY must be an absolute, release-bound build coordinate".into(),
        );
    }
    let temp = common::scratch("probierz-ugc-cli-first-use")?;
    let db = temp.join("ugc.sqlite3");
    let assets = temp.join("assets");
    let state_path = temp.join("onboarding.json");
    let env = BTreeMap::new();
    let run = |args: &[String]| invoke(&binary, args, &temp, &env, &db, &assets);
    let result = (|| {
        let mut view = run(&common::strings(&["onboarding"]))?;
        for (k, w) in [
            ("product_id", "ugc-cli"),
            ("journey_id", "first-use"),
            ("journey_version", "2026-08-04.1"),
            ("status", "in_progress"),
            ("screen_id", "campaign-system"),
        ] {
            if view[k] != w {
                return Err(format!("expected {k}={w}: {view}"));
            }
        }
        let mut state = common::read_json(&state_path)?;
        let progress = one(&state, "progress", "progress map")?;
        let attempt = progress["attempt_id"]
            .as_str()
            .ok_or("attempt id missing")?
            .to_string();
        let bundle = one(&state, "bundles", "bundle map")?.clone();
        if bundle["journey_version_id"] != "6aab4816-5057-4ea1-9acf-da233ecea9d4"
            || bundle["definition"]["first_success_fact"] != "campaign_record_created"
        {
            return Err("unexpected onboarding bundle".into());
        }
        view = run(&common::strings(&["onboarding", "next"]))?;
        if view["screen_id"] != "campaign-create" || view["status"] != "in_progress" {
            return Err("resume must advance to campaign-create".into());
        }
        view = run(&common::strings(&["onboarding", "next"]))?;
        if view["status"] != "in_progress" || !view["campaign_id"].is_null() {
            return Err(
                "navigation at the action boundary cannot satisfy campaign_record_created".into(),
            );
        }
        let campaign = run(&common::strings(&[
            "campaign",
            "create",
            "--name",
            "Probierz isolated first-use campaign",
            "--brand",
            "Probierz scenario brand",
            "--product",
            "UGC CLI verification product",
            "--objective",
            "Verify the durable campaign system of record",
            "--markets",
            "PL",
            "--languages",
            "pl",
            "--channels",
            "organic",
        ]))?;
        let id = campaign["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("campaign id must be a non-empty string")?
            .to_string();
        let persisted = run(&vec!["campaign".into(), "show".into(), id.clone()])?;
        for key in ["id", "name", "brand", "product", "objective"] {
            if persisted[key] != campaign[key] {
                return Err(format!("persisted campaign {key} differs"));
            }
        }
        view = run(&common::strings(&["onboarding"]))?;
        if view["status"] != "completed"
            || view["screen_id"] != "campaign-created"
            || view["campaign_id"] != id
        {
            return Err("real campaign record did not complete first use".into());
        }
        state = common::read_json(&state_path)?;
        let events = state["events"]
            .as_object()
            .ok_or("events missing")?
            .values()
            .collect::<Vec<_>>();
        let success = events
            .iter()
            .copied()
            .find(|e| e["event_name"] == "onboarding_first_success_observed")
            .ok_or("canonical first-success event must be retained by OfflineTransport")?;
        let revision = success["evidence_revision"]
            .as_str()
            .ok_or("evidence revision missing")?;
        common::write_json(
            &context
                .artifacts
                .join("ugc-cli-onboarding-first-use.trace.json"),
            &json!({"schemaVersion":1,"productId":"ugc-cli","journeyId":"first-use","journeyVersion":"2026-08-04.1","journeyVersionId":bundle["journey_version_id"],"sourceRevision":bundle["definition"]["source_revision"],"firstSuccessFact":"campaign_record_created","attemptId":attempt,"evidenceRevisionSha256":hex::encode(Sha256::digest(revision.as_bytes())),"completionEventId":success["event_id"],"observation":{"campaignIdSha256":hex::encode(Sha256::digest(id.as_bytes())),"persistedRecordMatched":true}}),
        )
    })();
    common::remove(&temp);
    result
}
