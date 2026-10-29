use crate::run::*;
use serde_json::json;
pub(crate) fn analyze_run(
    report_path: &Path,
    artifacts_dir: Option<&Path>,
    tool: Option<&str>,
    frames: Option<f64>,
    expected_run: Option<&str>,
) -> Result<Value, Failure> {
    let report = crate::json_file::read(report_path)?.ok_or_else(|| {
        fail(
            "run.analyze",
            format!(
                "report not found: {} (did the run produce one?)",
                report_path.display()
            ),
        )
    })?;
    let report_run = report.pointer("/probierz/runId").and_then(Value::as_str);
    if let Some(expected) = expected_run {
        if report_run != Some(expected) {
            return Err(fail(
                "run.analyze",
                match report_run {
                    Some(found) => {
                        format!("report run ID mismatch: expected {expected}, got {found}")
                    }
                    None => format!(
                        "report run ID mismatch: expected {expected}, the report states none"
                    ),
                },
            ));
        }
    }
    // A caller that names no tool has the report's own shape name it: a
    // probierz report carries its `probierz` block and `tests` list.
    let canonical =
        report.get("probierz").is_some() && report.get("tests").and_then(Value::as_array).is_some();
    let tool = match tool {
        Some(tool) => tool,
        None if canonical => "probierz",
        None => "wdio",
    };
    let mut summary = normalize_wdio(&report, tool);
    // normalize_wdio always lists the report's media, so a summary without
    // that list is refused rather than read as having none.
    let report_media = match summary
        .as_object_mut()
        .and_then(|summary| summary.remove("reportMedia"))
    {
        Some(Value::Array(media)) => media,
        other => {
            return Err(fail(
                "run.analyze",
                format!("the normalized report carries no media list: {other:?}"),
            ))
        }
    };
    let mut media = Vec::new();
    for item in report_media {
        let Some(file) = item.get("file").and_then(Value::as_str) else {
            continue;
        };
        let path = PathBuf::from(file);
        // Media whose report names no kind is listed with none (`null`).
        let kind = item.get("kind").and_then(Value::as_str);
        let mut entry = Map::new();
        entry.insert("file".into(), Value::String(file.into()));
        entry.insert("kind".into(), json!(kind));
        if let Some(content_type) = item.get("contentType").filter(|value| !value.is_null()) {
            entry.insert("contentType".into(), content_type.clone());
        }
        // A file that cannot be examined is said with its cause, not listed
        // as missing.
        match crate::paths::present(&path) {
            Ok(true) => {
                entry.insert("sizeKb".into(), json!(size_kb(&path)));
            }
            Ok(false) => {
                entry.insert("missing".into(), Value::Bool(true));
            }
            Err(cause) => {
                entry.insert("unexaminable".into(), Value::String(cause.to_string()));
            }
        }
        if kind == Some("video") {
            // Frames are extracted only when a positive count was stated; a
            // plan or command that states none asks for none.
            let extraction = frames.filter(|count| count.is_normal() && count.is_sign_positive());
            let extraction_requested = extraction.is_some();
            match probe_video(&path) {
                Ok(meta) => {
                    if let Some(frames) = extraction {
                        let artifacts = artifacts_dir.ok_or_else(|| {
                            fail(
                                "run.analyze",
                                "frame extraction requires an artifacts directory",
                            )
                        })?;
                        let extracted = extract_frames(&path, artifacts, frames, &meta)
                            .map_err(|error| fail("run.analyze", error))?;
                        entry.insert("frames".into(), json!(extracted));
                    }
                    entry.insert("recording".into(), meta);
                }
                Err(error) if extraction_requested => return Err(fail("run.analyze", error)),
                Err(error) => {
                    entry.insert("recordingError".into(), Value::String(error));
                }
            }
        }
        media.push(Value::Object(entry));
    }
    let mut timeline = None;
    let mut timeline_path = None;
    let mut diagnostics = None;
    if let Some(artifacts) = artifacts_dir {
        // The timeline hides what the run hid: the redaction it recorded. A
        // manifest that cannot be read or parsed is refused naming it; read as
        // an empty one it recorded no redaction and the timeline hid nothing.
        let recorded = crate::json_file::read(&artifacts.join("run-manifest.json"))?;
        let manifest = match &recorded {
            Some(recorded) => recorded,
            None => &Value::Null,
        };
        let redaction = manifest::Redaction::recorded(manifest);
        let built = build_timeline(
            &report,
            &summary,
            &media,
            artifacts,
            manifest.get("startedAt").and_then(Value::as_str),
            &redaction,
        );
        let path = artifacts.join("timeline.json");
        write_json(&path, &built)?;
        diagnostics = Some(summarize_diagnostics(
            &report, &built, artifacts, &redaction,
        ));
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
    let inventory: Vec<Value> = match artifacts_dir {
        Some(artifacts) => walk(artifacts, false)
            .into_iter()
            .filter(|file| file != report_path && !known.contains(file))
            .map(|file| json!({ "file": file, "sizeKb": size_kb(&file) }))
            .collect(),
        None => Vec::new(),
    };
    let mut output = match summary {
        Value::Object(summary) => summary,
        other => {
            return Err(fail(
                "run.analyze",
                format!("the normalized report is not an object: {other}"),
            ))
        }
    };
    // A report that names no run, no artifacts directory, no timeline or no
    // diagnostics carries `null` for it; capture errors it does not list are
    // none.
    output.insert("runId".into(), json!(report_run));
    output.insert("reportPath".into(), json!(report_path));
    output.insert("artifactsDir".into(), json!(artifacts_dir));
    let capture_errors = match report.pointer("/probierz/captureErrors") {
        Some(errors @ Value::Array(_)) => errors.clone(),
        None | Some(Value::Null) => json!([]),
        Some(other) => {
            return Err(fail(
                "run.analyze",
                format!("the report's captureErrors are not a list: {other}"),
            ))
        }
    };
    output.insert("captureErrors".into(), capture_errors);
    output.insert("media".into(), Value::Array(media));
    output.insert("artifacts".into(), Value::Array(inventory));
    let timeline_summary = match timeline.as_ref() {
        Some(timeline) => {
            json!({ "path": timeline_path, "counts": timeline["counts"], "diagnostics": timeline["diagnostics"] })
        }
        None => Value::Null,
    };
    output.insert("timeline".into(), timeline_summary);
    output.insert("diagnostics".into(), json!(diagnostics));
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
