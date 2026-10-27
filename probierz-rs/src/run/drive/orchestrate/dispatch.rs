use crate::failure::Code;
use crate::gate::evidence_rank;
use crate::run::*;
use serde_json::json;

use super::cell::{arguments, artifact_key, cell_level, declared_env, extend, Arguments};

/// The edition of the matrix report's shape, which its readers compare
/// before they read it: https://json-schema.org/understanding-json-schema/reference/schema#schema
const MATRIX_SCHEMA_VERSION: u64 = 1;

pub fn matrix(harness: &Path, app_id: &str, profile: &str, args: &[String]) -> Answer {
    let Arguments {
        plan_only,
        release,
        mut env,
    } = arguments(args)?;
    if profile == "release" && !plan_only && release.is_none() {
        return Err(fail(
            "cli.matrix",
            "release matrix execution needs --release <id>",
        ));
    }
    let plan = plan_matrix(harness, app_id, profile)?;
    if plan_only {
        return print_json(&plan);
    }
    let effective = match release {
        Some(release) => Some(release),
        None => plan
            .get("release")
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    if profile == "release" && effective.is_none() {
        return Err(fail(
            "run.matrix",
            "release matrix execution needs a release ID",
        ));
    }
    let artifact_key = artifact_key(&env)?;
    if plan.get("artifactEncryption").and_then(Value::as_str) == Some("required")
        && artifact_key.is_none()
    {
        return Err(fail(
            "run.matrix",
            "matrix requires PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE",
        ));
    }
    env.remove("PROBIERZ_ARTIFACT_ENCRYPTION_KEY_FILE");
    let required = plan
        .get("minimumCellEvidence")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            fail(
                "run.matrix",
                format!("matrix {profile} plan states no minimumCellEvidence; the manifest profile must state E2 or E3"),
            )
        })?;
    // The one rank the gate, status, receipts and publication read.
    let required_rank = evidence_rank(required).ok_or_else(|| {
        fail(
            "run.matrix",
            format!("matrix {profile} plan's minimumCellEvidence {required:?} is not an evidence level the gate reads (E followed by its rank)"),
        )
    })?;
    let Some(record) = plan.get("record").and_then(Value::as_bool) else {
        return Err(fail(
            "run.matrix",
            format!("matrix {profile} plan states no record setting"),
        ));
    };
    let remove_plaintext = plan
        .get("removePlaintextAfterProtection")
        .and_then(Value::as_bool)
        == Some(true);
    let Some(cells) = plan.get("cells").and_then(Value::as_array) else {
        return Err(fail(
            "run.matrix",
            format!("matrix {profile} plan states no cells"),
        ));
    };
    for cell in cells {
        for (name, value) in &env {
            if let Some(axis) = cell
                .get("axes")
                .and_then(|axes| axes.get(name))
                .and_then(Value::as_str)
            {
                if axis != value {
                    return Err(fail(
                        "run.matrix",
                        format!("matrix axis {name} cannot be overridden"),
                    ));
                }
            }
        }
    }

    let protect = |run: &Value| -> (Value, Value) {
        let Some(key) = artifact_key.as_deref() else {
            return (Value::Null, Value::Null);
        };
        let Some(run_id) = run.get("runId").and_then(Value::as_str) else {
            return (
                Value::Null,
                Value::String("the run states no runId; its artifacts cannot be protected".into()),
            );
        };
        let result = crate::evidence::protect_run(
            harness,
            app_id,
            run_id,
            Some(profile),
            Some(Path::new(key)),
            remove_plaintext,
        );
        match result {
            Ok(artifact) => (artifact, Value::Null),
            Err(error) => (Value::Null, Value::String(error.to_string())),
        }
    };

    let redaction = crate::manifest::Redaction::of_app(harness, Some(app_id))?;
    let mut results = Vec::new();
    for cell in cells {
        let mut cell_env = declared_env(cell)?;
        cell_env.extend(env.clone());
        if let Some(release) = &effective {
            cell_env.insert("PROBIERZ_RELEASE".into(), release.clone());
        }
        let run = run_surface(
            harness,
            cell.get("target").and_then(Value::as_str).ok_or_else(|| {
                fail(
                    "run.matrix",
                    format!("a matrix cell states no target: {cell}"),
                )
            })?,
            RunOptions {
                env: cell_env,
                record,
                force: false,
                spec: cell.get("spec").and_then(Value::as_str).map(str::to_string),
                app_id: Some(app_id.into()),
                kind: Some(profile.into()),
            },
        )?;
        let mut public = cell.clone();
        if let Some(map) = public.get_mut("env").and_then(Value::as_object_mut) {
            for (name, value) in map.iter_mut() {
                if redaction.covers(name) {
                    *value = json!("[REDACTED]");
                }
            }
        }
        let canceled = run.get("canceled").and_then(Value::as_bool) == Some(true);
        if canceled || run.get("skipped").and_then(Value::as_bool) == Some(true) {
            let (artifact, protection_error) = protect(&run);
            extend(
                &mut public,
                json!({
                    "status": if canceled { "canceled" } else { "blocked" },
                    "run": run,
                    "protectedArtifact": artifact,
                    "protectionError": protection_error,
                }),
            )?;
            results.push(public);
            continue;
        }

        let (analysis, completed) =
            analyze_and_complete(run, plan.get("frames").and_then(Value::as_f64))?;
        let (artifact, protection_error) = protect(&completed);
        let passed = completed.get("passed").and_then(Value::as_bool) == Some(true)
            && protection_error.is_null();
        extend(
            &mut public,
            json!({
                "status": if passed { "passed" } else { "failed" },
                "evidenceLevel": cell_level(&completed),
                "run": completed,
                "analysis": analysis,
                "protectedArtifact": artifact,
                "protectionError": protection_error,
            }),
        )?;
        results.push(public);
    }

    let count = |status: &str| {
        results
            .iter()
            .filter(|result| result.get("status").and_then(Value::as_str) == Some(status))
            .count()
    };
    // A cell that was blocked or canceled reached no evidence level, so it
    // cannot satisfy the plan's minimum.
    let evidence_satisfied = results.iter().all(|result| {
        match result
            .get("evidenceLevel")
            .and_then(Value::as_str)
            .and_then(evidence_rank)
        {
            Some(rank) => rank >= required_rank,
            None => false,
        }
    });
    let passed = count("passed") == results.len() && evidence_satisfied;
    let output = json!({
        "schemaVersion": MATRIX_SCHEMA_VERSION,
        "appId": app_id,
        "profile": profile,
        "release": effective,
        "generatedAt": now_iso(),
        "verdict": {
            "passed": passed,
            "evidenceLevel": if passed { json!("E4") } else { Value::Null },
            "minimumCellEvidence": required,
            "evidenceSatisfied": evidence_satisfied,
        },
        "summary": {
            "total": results.len(),
            "passed": count("passed"),
            "failed": count("failed"),
            "blocked": count("blocked"),
            "canceled": count("canceled"),
        },
        "results": results,
    });
    print_json(&output)?;
    passed
        .then_some(())
        .ok_or_else(|| Failure::new("run.matrix", Code::Refused, output.to_string()))
}
