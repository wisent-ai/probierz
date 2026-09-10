use serde_json::json;
use crate::authoring::*;
pub(crate) fn patch_paths(patch: &str) -> Result<Vec<String>, String> {
    if patch.trim().is_empty() {
        return Err("product_patch needs a non-empty patch".to_string());
    }
    if patch.len() > MAX_PATCH_CHARS {
        return Err(format!("patch exceeds {MAX_PATCH_CHARS} characters"));
    }
    let mut files = BTreeSet::new();
    for line in patch
        .lines()
        .filter(|line| line.starts_with("diff --git a/"))
    {
        let rest = &line[11..];
        if let Some((left, right)) = rest.split_once(" b/") {
            files.insert(left.to_string());
            files.insert(right.to_string());
        }
    }
    if files.is_empty() {
        return Err("patch must be a git unified diff".to_string());
    }
    if files.len() > MAX_CHANGED_FILES {
        return Err(format!(
            "patch changes {} files; limit is {MAX_CHANGED_FILES}",
            files.len()
        ));
    }
    for file in &files {
        let lower = file.to_ascii_lowercase();
        let denied_component = lower.split('/').any(|part| {
            part == ".stado"
                || part == ".github"
                || part == ".gitlab"
                || part == "node_modules"
                || part == "test-results"
                || part == "deploy"
                || part == "infra"
                || part == "terraform"
                || part.starts_with(".env")
                || part.contains("credential")
                || part.contains("secret")
                || matches!(
                    part,
                    "agents.md"
                        | "dockerfile"
                        | "cargo.lock"
                        | "package-lock.json"
                        | "pnpm-lock.yaml"
                        | "yarn.lock"
                        | "poetry.lock"
                        | "pipfile.lock"
                        | "id_rsa"
                )
                || part.ends_with(".pem")
                || part.ends_with(".key")
                || part.ends_with(".p12")
        });
        if Path::new(file).is_absolute()
            || file.split('/').any(|part| part == "..")
            || denied_component
        {
            return Err(format!("patch may not change {file}"));
        }
    }
    Ok(files.into_iter().collect())
}

pub(crate) fn repair_failure(
    source_run_id: Option<&str>,
    code: &str,
    retryable: bool,
    detail: impl Into<String>,
    message: impl Into<String>,
) -> JsonValue {
    json!({
        "ok": false,
        "sourceRunId": source_run_id,
        "failure": {
            "failure_point": "repair.dispatch",
            "error_code": code,
            "retryable": retryable,
            "detail": detail.into(),
            "message": message.into()
        }
    })
}

pub(crate) fn read_json_value(path: &Path) -> Option<JsonValue> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

pub(crate) fn write_pretty_json(path: &Path, value: &JsonValue) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, format!("{text}\n")).map_err(|error| error.to_string())
}

pub(crate) fn repair_source_run(
    harness: &Path,
    app_id: &str,
    requested: Option<&str>,
) -> Result<JsonValue, JsonValue> {
    let history =
        crate::status::run_history_value(harness, app_id, None, 100).map_err(|error| {
            repair_failure(
                None,
                error.code.as_str(),
                error.code.retryable(),
                error.detail,
                "Automated repair failed.",
            )
        })?;
    let runs = history
        .get("runs")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let run = if let Some(run_id) = requested {
        runs.into_iter()
            .find(|run| run.get("runId").and_then(JsonValue::as_str) == Some(run_id))
    } else {
        runs.into_iter()
            .find(|run| run.get("status").and_then(JsonValue::as_str) == Some("failed"))
    };
    let Some(run) = run else {
        let (detail, message) = if let Some(run_id) = requested {
            (
                format!("run {run_id} was not found"),
                format!("Run {run_id} was not found."),
            )
        } else {
            (
                format!("no failed run recorded for {app_id}"),
                format!("No failed run is recorded for {app_id}."),
            )
        };
        return Err(repair_failure(None, "not_found", false, detail, message));
    };
    let run_id = run
        .get("runId")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let status = run
        .get("status")
        .and_then(JsonValue::as_str)
        .unwrap_or("unknown");
    if status != "failed" {
        return Err(repair_failure(
            Some(run_id),
            "config",
            false,
            format!("run {run_id} has status {status}"),
            format!("Run {run_id} is {status}; only failed runs are repairable."),
        ));
    }
    if run.get("failureClass").and_then(JsonValue::as_str) == Some("infrastructure") {
        return Err(repair_failure(
            Some(run_id), "infra_down", true,
            format!("run {run_id} failed before product behavior could be observed"),
            format!("Run {run_id} is an infrastructure failure; repair the host or toolchain instead of product code."),
        ));
    }
    Ok(run)
}

pub(crate) fn find_named_file(root: &Path, name: &str) -> Option<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(directory).ok()?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().and_then(OsStr::to_str) == Some(name) {
                return Some(path);
            }
        }
    }
    None
}

pub(crate) fn recorded_spec_path(harness: &Path, run: &JsonValue) -> Option<PathBuf> {
    let name = Path::new(run.get("spec")?.as_str()?)
        .file_name()?
        .to_str()?;
    let target = run.get("target")?.as_str()?;
    find_named_file(&target_spec_dir(harness, target)?, name)
}

pub(crate) fn repair_evidence(run: &JsonValue) -> JsonValue {
    let directory = run
        .get("manifestPath")
        .and_then(JsonValue::as_str)
        .and_then(|file| Path::new(file).parent())
        .map(Path::to_path_buf);
    let analysis = run
        .get("analysisPath")
        .and_then(JsonValue::as_str)
        .filter(|value| !value.is_empty())
        .and_then(|file| read_json_value(Path::new(file)))
        .or_else(|| {
            directory
                .as_ref()
                .and_then(|directory| read_json_value(&directory.join("analysis.json")))
        });
    let report = directory
        .as_ref()
        .and_then(|directory| read_json_value(&directory.join("report.json")));
    let failures = analysis
        .as_ref()
        .and_then(|value| value.get("failures"))
        .and_then(JsonValue::as_array)
        .or_else(|| {
            report
                .as_ref()
                .and_then(|value| value.get("failures"))
                .and_then(JsonValue::as_array)
        })
        .into_iter()
        .flatten()
        .filter_map(|failure| {
            let detail = failure
                .get("error")
                .or_else(|| failure.get("message"))
                .or_else(|| failure.get("detail"))
                .and_then(JsonValue::as_str)
                .unwrap_or_default()
                .chars()
                .take(1200)
                .collect::<String>();
            if detail.is_empty() {
                return None;
            }
            let title = failure
                .get("title")
                .or_else(|| failure.get("test"))
                .and_then(JsonValue::as_str)
                .unwrap_or("failure")
                .chars()
                .take(200)
                .collect::<String>();
            Some(json!({ "title": title, "detail": detail }))
        })
        .take(8)
        .collect::<Vec<_>>();
    json!({ "failures": failures, "analysis": analysis.as_ref().and_then(|value| value.get("summary")).cloned().unwrap_or(JsonValue::Null) })
}

