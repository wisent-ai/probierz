use serde_json::json;
use crate::run::*;
pub(crate) fn analyze_run(
    report_path: &Path,
    artifacts_dir: Option<&Path>,
    tool: Option<&str>,
    frames: f64,
    expected_run: Option<&str>,
) -> Result<Value, Failure> {
    if !report_path.exists() {
        return Err(fail(
            "run.analyze",
            format!(
                "report not found: {} (did the run produce one?)",
                report_path.display()
            ),
        ));
    }
    let report: Value = serde_json::from_slice(&fs::read(report_path)?)
        .map_err(|error| Failure::config("run.analyze", error.to_string()))?;
    let report_run = report.pointer("/probierz/runId").and_then(Value::as_str);
    if let Some(expected) = expected_run {
        if report_run != Some(expected) {
            return Err(fail(
                "run.analyze",
                format!(
                    "report run ID mismatch: expected {expected}, got {}",
                    report_run.unwrap_or("missing")
                ),
            ));
        }
    }
    let canonical =
        report.get("probierz").is_some() && report.get("tests").and_then(Value::as_array).is_some();
    let playwright = report.get("suites").and_then(Value::as_array).is_some();
    let mut summary = if canonical {
        normalize_wdio(&report, tool.unwrap_or("probierz"))
    } else if playwright {
        normalize_playwright(&report)
    } else {
        normalize_wdio(&report, tool.unwrap_or("wdio"))
    };
    let report_media = summary
        .as_object_mut()
        .expect("object")
        .remove("reportMedia")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let mut media = Vec::new();
    for item in report_media {
        let Some(file) = item.get("file").and_then(Value::as_str) else {
            continue;
        };
        let path = PathBuf::from(file);
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
        let mut entry = Map::new();
        entry.insert("file".into(), Value::String(file.into()));
        entry.insert("kind".into(), Value::String(kind.into()));
        if let Some(content_type) = item.get("contentType").filter(|value| !value.is_null()) {
            entry.insert("contentType".into(), content_type.clone());
        }
        if path.exists() {
            entry.insert("sizeKb".into(), json!(size_kb(&path)));
            if kind == "video" {
                if let Some(meta) = probe_video(&path) {
                    entry.insert("recording".into(), meta);
                }
                if frames > 0.0 {
                    if let Some(artifacts) = artifacts_dir {
                        entry.insert(
                            "frames".into(),
                            json!(extract_frames(&path, artifacts, frames)),
                        );
                    }
                }
            }
        } else {
            entry.insert("missing".into(), Value::Bool(true));
        }
        media.push(Value::Object(entry));
    }
    let mut timeline = None;
    let mut timeline_path = None;
    let mut diagnostics = None;
    if let Some(artifacts) = artifacts_dir {
        let manifest: Value = fs::read(artifacts.join("run-manifest.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| json!({}));
        let built = build_timeline(
            &report,
            &summary,
            &media,
            artifacts,
            manifest.get("startedAt").and_then(Value::as_str),
        );
        let path = artifacts.join("timeline.json");
        write_json(&path, &built)?;
        diagnostics = Some(summarize_diagnostics(&report, &built, artifacts));
        timeline_path = Some(path);
        timeline = Some(built);
    }
    let known: BTreeSet<PathBuf> = media
        .iter()
        .filter_map(|item| item.get("file").and_then(Value::as_str).map(PathBuf::from))
        .chain(timeline_path.clone())
        .chain(
            diagnostics
                .as_ref()
                .and_then(|value| value.get("file"))
                .and_then(Value::as_str)
                .map(PathBuf::from),
        )
        .collect();
    let inventory: Vec<Value> = artifacts_dir
        .map(|artifacts| {
            walk(artifacts, false)
                .into_iter()
                .filter(|file| file != report_path && !known.contains(file))
                .map(|file| json!({ "file": file, "sizeKb": size_kb(&file) }))
                .collect()
        })
        .unwrap_or_default();
    let mut output = summary.as_object().expect("object").clone();
    output.insert(
        "runId".into(),
        report_run
            .map(|value| Value::String(value.into()))
            .unwrap_or(Value::Null),
    );
    output.insert("reportPath".into(), json!(report_path));
    output.insert(
        "artifactsDir".into(),
        artifacts_dir.map(|path| json!(path)).unwrap_or(Value::Null),
    );
    output.insert(
        "captureErrors".into(),
        report
            .pointer("/probierz/captureErrors")
            .filter(|value| value.is_array())
            .cloned()
            .unwrap_or_else(|| json!([])),
    );
    output.insert("media".into(), Value::Array(media));
    output.insert("artifacts".into(), Value::Array(inventory));
    output.insert("timeline".into(), timeline.as_ref().map(|timeline| json!({ "path": timeline_path, "counts": timeline["counts"], "diagnostics": timeline["diagnostics"] })).unwrap_or(Value::Null));
    output.insert("diagnostics".into(), diagnostics.unwrap_or(Value::Null));
    Ok(Value::Object(output))
}

pub fn analyze(_harness: &Path, report: &str, args: &[String]) -> Answer {
    let opts = parse_run_args(args, true)?;
    let artifacts = args
        .first()
        .filter(|arg| !arg.starts_with("--"))
        .map(PathBuf::from);
    let result = analyze_run(
        Path::new(report),
        artifacts.as_deref(),
        opts.tool.as_deref(),
        opts.frames,
        None,
    )?;
    print_json(&result)
}

