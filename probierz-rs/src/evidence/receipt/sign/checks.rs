//! What a receipt checks before it is signed: every run's artifacts and secret
//! scan, every run's identities and evidence level, that the runs name one exact
//! build per target, and that the required journeys were covered.

use crate::evidence::*;
use serde_json::json;

/// Verifies each run's artifacts (or its protected bundle) and secret scan, pushing
/// every problem to `errors`; returns the normalised scan per run id.
pub(super) fn check_artifacts(
    harness: &Path,
    app_id: &str,
    source_runs: &[Value],
    normalized: &[Value],
    errors: &mut Vec<String>,
) -> Result<Map<String, Value>, Failure> {
    let mut scans = Map::new();
    for (source, signed) in source_runs.iter().zip(normalized) {
        let run_id = signed
            .get("runId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let artifact_root = PathBuf::from(
            source
                .get("manifestPath")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        let artifact_root = artifact_root.parent().unwrap_or(Path::new(""));
        if source
            .pointer("/protection/plaintextRemoved")
            .and_then(Value::as_bool)
            == Some(true)
        {
            let protected_root =
                absolute(&harness.join("test-results").join(".protected").join(app_id))?;
            let bundle = absolute(Path::new(
                source
                    .pointer("/protection/file")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ))?;
            if !bundle.starts_with(&protected_root) || !bundle.is_file() {
                errors.push(format!(
                    "{run_id}: protected artifact is missing or escapes its product root"
                ));
            } else if sha256_file(&bundle)?
                != signed
                    .pointer("/protection/sha256")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            {
                errors.push(format!("{run_id}: protected artifact hash mismatch"));
            }
        } else {
            for artifact in signed
                .get("artifacts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let relative = artifact
                    .get("file")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let file = absolute(&artifact_root.join(relative))?;
                let root = absolute(artifact_root)?;
                if !file.starts_with(&root) || !file.is_file() {
                    errors.push(format!(
                        "{run_id}: artifact is missing or escapes its run: {relative}"
                    ));
                } else if sha256_file(&file)?
                    != artifact
                        .get("sha256")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                {
                    errors.push(format!("{run_id}: artifact hash mismatch: {relative}"));
                }
            }
        }
        let scan = if source
            .pointer("/protection/plaintextRemoved")
            .and_then(Value::as_bool)
            == Some(true)
        {
            source.pointer("/protection/secretScan").cloned()
        } else {
            Some(scan_secrets(artifact_root)?)
        };
        let normalized_scan = scan.as_ref().map(|scan| json!({
            "passed": scan.get("passed").and_then(Value::as_bool).unwrap_or(false),
            "scannedAt": scan.get("scannedAt").cloned().unwrap_or(Value::Null),
            "scannedFiles": scan.get("scannedFiles").and_then(Value::as_f64).map(js_number).unwrap_or_else(|| json!(0)),
            "skippedBinary": scan.get("skippedBinary").and_then(Value::as_f64).map(js_number).unwrap_or_else(|| json!(0)),
            "findings": scan.get("findings").cloned().filter(Value::is_array).unwrap_or_else(|| json!([])),
        })).unwrap_or(Value::Null);
        if scan
            .as_ref()
            .and_then(|scan| scan.get("passed"))
            .and_then(Value::as_bool)
            != Some(true)
        {
            errors.push(format!(
                "{run_id}: plaintext secret scan is missing or has findings"
            ));
        }
        scans.insert(run_id.to_string(), normalized_scan);
    }
    Ok(scans)
}

fn levels(name: &str) -> i32 {
    match name {
        "E0" => 0,
        "E1" => 1,
        "E2" => 2,
        "E3" => 3,
        "E4" => 4,
        "E5" => 5,
        _ => -1,
    }
}

/// Verifies each run's status, harness and source identities, build hash, artifact
/// hashes and evidence level against what the receipt expects.
pub(super) fn check_runs(
    normalized: &[Value],
    expected_harness: &str,
    expected_source: &str,
    minimum: &str,
    errors: &mut Vec<String>,
) {
    for run in normalized {
        let run_id = run.get("runId").and_then(Value::as_str).unwrap_or_default();
        let status = run
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if status != "passed" {
            errors.push(format!("{run_id}: status {status}"));
        }
        if run.pointer("/harness/sha256").and_then(Value::as_str) != Some(expected_harness)
            || run
                .pointer("/harness/gitSha")
                .and_then(Value::as_str)
                .is_none_or(|value| !is_git_sha(value))
            || run
                .pointer("/harness/worktreeSha256")
                .and_then(Value::as_str)
                .is_none_or(|value| !is_sha256(value))
        {
            errors.push(format!(
                "{run_id}: harness source identity mismatch or incomplete"
            ));
        }
        let bad_source = run.pointer("/source/sha256").and_then(Value::as_str)
            != Some(expected_source)
            || run
                .pointer("/source/repositories")
                .and_then(Value::as_array)
                .is_none_or(|repositories| {
                    repositories.iter().any(|repository| {
                        repository
                            .get("gitSha")
                            .and_then(Value::as_str)
                            .is_none_or(|value| !is_git_sha(value))
                            || repository
                                .get("worktreeSha256")
                                .and_then(Value::as_str)
                                .is_none_or(|value| !is_sha256(value))
                    })
                });
        if bad_source {
            errors.push(format!(
                "{run_id}: app source identity mismatch or incomplete"
            ));
        }
        if run
            .pointer("/build/sha256")
            .and_then(Value::as_str)
            .is_none_or(|value| !is_sha256(value))
        {
            errors.push(format!("{run_id}: build hash missing or invalid"));
        }
        if run
            .get("artifacts")
            .and_then(Value::as_array)
            .is_none_or(|artifacts| {
                artifacts.is_empty()
                    || artifacts.iter().any(|artifact| {
                        artifact
                            .get("sha256")
                            .and_then(Value::as_str)
                            .is_none_or(|value| !is_sha256(value))
                    })
            })
        {
            errors.push(format!("{run_id}: artifact hashes incomplete or invalid"));
        }
        let level = run
            .get("evidenceLevel")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if levels(level) < levels(minimum) {
            errors.push(format!("{run_id}: {level} is below {minimum}"));
        }
    }
}

/// The one build hash per target the runs identify; a target with two hashes is an error.
pub(super) fn exact_builds(normalized: &[Value], errors: &mut Vec<String>) -> Map<String, Value> {
    let mut builds = Map::new();
    for run in normalized {
        let Some(hash) = run.pointer("/build/sha256").and_then(Value::as_str) else {
            continue;
        };
        let target = run
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if builds
            .get(target)
            .and_then(Value::as_str)
            .is_some_and(|old| old != hash)
        {
            errors.push(format!("{target}: runs do not identify one exact build"));
        } else {
            builds.insert(target.to_string(), json!(hash));
        }
    }
    builds
}

/// The journeys the passed runs covered, the ones the receipt requires, and the
/// difference, which is pushed to `errors`.
pub(super) fn journey_coverage(
    normalized: &[Value],
    journeys_csv: Option<&str>,
    errors: &mut Vec<String>,
) -> (BTreeSet<String>, BTreeSet<String>, Vec<String>) {
    let covered: BTreeSet<String> = normalized
        .iter()
        .filter(|run| run.get("status").and_then(Value::as_str) == Some("passed"))
        .flat_map(|run| {
            run.get("journeys")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let required: BTreeSet<String> = journeys_csv
        .unwrap_or_default()
        .split(',')
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    let missing: Vec<String> = required.difference(&covered).cloned().collect();
    errors.extend(
        missing
            .iter()
            .map(|journey| format!("missing journey: {journey}")),
    );
    (covered, required, missing)
}
