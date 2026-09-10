use crate::specs::*;
pub(crate) fn panic_text(panic: Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    if let Some(text) = panic.downcast_ref::<String>() {
        return text.clone();
    }
    "unknown panic".to_string()
}

pub(crate) fn write_report(path: &Path, report: &Value) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| fail("specs.report", format!("{}: {error}", parent.display())))?;
    }
    let mut body = serde_json::to_vec_pretty(report)
        .map_err(|error| fail("specs.report", error.to_string()))?;
    body.push(b'\n');
    let mut file = create_private(path)
        .map_err(|error| fail("specs.report", format!("{}: {error}", path.display())))?;
    file.write_all(&body)
        .map_err(|error| fail("specs.report", format!("{}: {error}", path.display())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("probierz-specs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn passing(_context: &Context) -> Result<(), String> {
        Ok(())
    }

    fn failing(_context: &Context) -> Result<(), String> {
        Err("the screen never showed the first-use banner".to_string())
    }

    fn panicking(_context: &Context) -> Result<(), String> {
        panic!("driver disappeared");
    }

    fn run_rows(specs: Vec<Spec>, dir: &Path) -> Value {
        let mut rows = Vec::new();
        let mut capture_errors: Vec<String> = Vec::new();
        for spec in &specs {
            let context = Context {
                harness: dir.to_path_buf(),
                artifacts: dir.to_path_buf(),
                title: spec.title.to_string(),
                env: BTreeMap::new(),
                media: Mutex::new(Vec::new()),
            };
            let outcome = catch_unwind(AssertUnwindSafe(|| (spec.run)(&context)));
            let error = match outcome {
                Ok(Ok(())) => None,
                Ok(Err(reason)) => Some(reason),
                Err(panic) => Some(format!("journey panicked: {}", panic_text(panic))),
            };
            match validate_media(dir, &context.declared_media()) {
                Ok(_) => {}
                Err(reason) => capture_errors.push(format!("{}: {reason}", spec.title)),
            }
            rows.push(json!({ "title": spec.title, "error": error }));
        }
        json!({ "rows": rows, "captureErrors": capture_errors })
    }

    #[test]
    fn a_failing_journey_reports_its_reason_and_does_not_stop_the_surface() {
        let dir = scratch("continue");
        let report = run_rows(
            vec![
                Spec {
                    surface: "tui",
                    title: "first",
                    run: failing,
                },
                Spec {
                    surface: "tui",
                    title: "second",
                    run: passing,
                },
            ],
            &dir,
        );
        assert_eq!(
            report["rows"][0]["error"],
            "the screen never showed the first-use banner"
        );
        assert_eq!(report["rows"][1]["error"], Value::Null);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_panicking_journey_is_a_failed_row_not_a_lost_run() {
        let dir = scratch("panic");
        let report = run_rows(
            vec![
                Spec {
                    surface: "tui",
                    title: "boom",
                    run: panicking,
                },
                Spec {
                    surface: "tui",
                    title: "after",
                    run: passing,
                },
            ],
            &dir,
        );
        assert!(
            report["rows"][0]["error"]
                .as_str()
                .expect("reason")
                .contains("driver disappeared"),
            "row: {}",
            report["rows"][0]
        );
        assert_eq!(report["rows"][1]["error"], Value::Null);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn media_outside_the_artifacts_directory_is_refused() {
        let dir = scratch("media");
        let outside =
            std::env::temp_dir().join(format!("probierz-outside-{}.png", std::process::id()));
        fs::write(&outside, b"x").expect("outside file");
        let path = outside.clone();
        let context = Context {
            harness: dir.clone(),
            artifacts: dir.clone(),
            title: "escape".to_string(),
            env: BTreeMap::new(),
            media: Mutex::new(Vec::new()),
        };
        context.media("screenshot", path);
        let error = validate_media(&dir, &context.declared_media()).expect_err("must refuse");
        assert_eq!(error, "media path escapes the artifacts directory");
        let _ = fs::remove_file(&outside);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn declared_media_that_does_not_exist_is_refused() {
        let dir = scratch("missing-media");
        let context = Context {
            harness: dir.clone(),
            artifacts: dir.clone(),
            title: "ghost".to_string(),
            env: BTreeMap::new(),
            media: Mutex::new(Vec::new()),
        };
        context.media("screenshot", dir.join("never-written.png"));
        let error = validate_media(&dir, &context.declared_media()).expect_err("must refuse");
        assert!(
            error.starts_with("declared media does not exist:"),
            "error: {error}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_variable_names_itself_and_what_it_must_point_at() {
        let dir = scratch("required");
        let context = Context {
            harness: dir.clone(),
            artifacts: dir.clone(),
            title: "needs".to_string(),
            env: BTreeMap::from([("EMPTY".to_string(), "   ".to_string())]),
            media: Mutex::new(Vec::new()),
        };
        let error = context
            .required("EMPTY", "the released Brama executable")
            .expect_err("must refuse");
        assert!(
            error.starts_with("EMPTY is required: the released Brama executable"),
            "error: {error}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_reason_keeps_its_headline_and_its_final_state() {
        let text = format!(
            "EXPECTED: the banner\n{}\nSCREEN: last frame",
            "x".repeat(4000)
        );
        let clipped = clip_row_error(&text);
        assert!(
            clipped.starts_with("EXPECTED: the banner"),
            "clipped: {}",
            &clipped[..40]
        );
        assert!(
            clipped.ends_with("SCREEN: last frame"),
            "clipped tail missing"
        );
        assert!(clipped.contains("\n...\n"), "elision marker missing");
    }
}
