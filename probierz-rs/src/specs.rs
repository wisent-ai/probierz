//! The journeys this toolkit runs itself, and the runner that reports them.
//!
//! Two surfaces execute here rather than through a browser driver: terminal
//! applications, driven over a real PTY, and native desktop applications,
//! driven through the accessibility tree. Both used to be Node processes that
//! a Node runner spawned one per journey. They are functions now, and the
//! runner calls them, so a journey failure is a returned reason instead of a
//! child process exit status.
//!
//! The report this writes is the canonical one: the same shape the Playwright
//! reporter emits, so `analyze` treats every surface alike.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use serde_json::{json, Value};

use crate::failure::{create_private, fail, iso_timestamp, Failure};

pub mod cua;
pub mod tui;

/// One journey: a name an operator reads in the report, and the function that
/// performs it. A journey answers with the reason it failed, never a panic —
/// the runner still catches those, because a panic in one journey must not
/// take the rest of the surface with it.
pub struct Spec {
    pub surface: &'static str,
    pub title: &'static str,
    pub run: fn(&Context) -> Result<(), String>,
}

/// What a journey is given: where artifacts go, and the environment the
/// operator provisioned. Nothing is invented here — a journey that needs a
/// binary, a model, or an account says which variable is missing and stops.
pub struct Context {
    pub harness: PathBuf,
    pub artifacts: PathBuf,
    pub title: String,
    env: BTreeMap<String, String>,
    media: Mutex<Vec<Media>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Media {
    pub file: PathBuf,
    pub kind: &'static str,
    #[serde(rename = "contentType", skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

impl Context {
    /// A variable the operator must have provisioned. The message names the
    /// variable and what it has to point at, because a journey cannot create
    /// a real account, a real subscription, or a released binary.
    pub fn required(&self, name: &str, what: &str) -> Result<String, String> {
        match self.env.get(name).map(|value| value.trim().to_string()) {
            Some(value) if !value.is_empty() => Ok(value),
            _ => Err(format!(
                "{name} is required: {what}; Probierz never invents or provisions provider access"
            )),
        }
    }

    /// A variable a journey may use when it is set.
    pub fn optional(&self, name: &str) -> Option<String> {
        self.env
            .get(name)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    /// Record a screenshot, trace, or video this journey produced. The runner
    /// refuses paths outside the artifacts directory and files that are not
    /// there, so a report never points at evidence that does not exist.
    pub fn media(&self, kind: &'static str, file: impl Into<PathBuf>) {
        let entry = Media {
            file: file.into(),
            kind,
            content_type: None,
        };
        self.media.lock().expect("media lock").push(entry);
    }

    pub fn media_typed(&self, kind: &'static str, file: impl Into<PathBuf>, content_type: &str) {
        let entry = Media {
            file: file.into(),
            kind,
            content_type: Some(content_type.to_string()),
        };
        self.media.lock().expect("media lock").push(entry);
    }

    fn declared_media(&self) -> Vec<Media> {
        self.media.lock().expect("media lock").clone()
    }
}

/// Every journey this toolkit owns, in the order a report lists them.
pub fn registry() -> Vec<Spec> {
    let mut all = Vec::new();
    all.extend(tui::specs());
    all.extend(cua::specs());
    all.sort_by(|left, right| (left.surface, left.title).cmp(&(right.surface, right.title)));
    all
}

/// The journeys of one surface, optionally narrowed to a title or a prefix.
pub fn select(surface: &str, filter: Option<&str>) -> Vec<Spec> {
    registry()
        .into_iter()
        .filter(|spec| spec.surface == surface)
        .filter(|spec| match filter {
            None => true,
            Some(want) => match want.strip_suffix('*') {
                Some(prefix) => spec.title.starts_with(prefix),
                None => spec.title == want,
            },
        })
        .collect()
}

fn at_iso(base: SystemTime, elapsed: Duration) -> String {
    iso_timestamp(base + elapsed)
}

/// An operator reads these lines on a terminal: keep the headline that says
/// what was expected and the tail that says what the application was showing.
/// A long screen dump never drowns the reason.
fn clip_row_error(text: &str) -> String {
    const LIMIT: usize = 2000;
    const HEAD: usize = 600;
    const TAIL: usize = 1400;
    if text.chars().count() <= LIMIT {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let head: String = chars[..HEAD].iter().collect();
    let tail: String = chars[chars.len() - TAIL..].iter().collect();
    format!("{head}\n...\n{tail}")
}

fn validate_media(artifacts: &Path, declared: &[Media]) -> Result<Vec<Media>, String> {
    let root = fs::canonicalize(artifacts).unwrap_or_else(|_| artifacts.to_path_buf());
    let mut out = Vec::with_capacity(declared.len());
    for entry in declared {
        if !["screenshot", "trace", "video"].contains(&entry.kind) {
            return Err(format!("unsupported media kind {}", entry.kind));
        }
        let resolved = fs::canonicalize(&entry.file)
            .map_err(|_| format!("declared media does not exist: {}", entry.file.display()))?;
        if resolved != root && !resolved.starts_with(&root) {
            return Err("media path escapes the artifacts directory".to_string());
        }
        out.push(Media {
            file: resolved,
            kind: entry.kind,
            content_type: entry.content_type.clone(),
        });
    }
    Ok(out)
}

/// Run one surface's journeys and write the canonical report.
///
/// The exit code is the operator's answer: zero when every journey passed.
pub fn execute(
    surface: &str,
    harness: &Path,
    artifacts: &Path,
    report_path: &Path,
    filter: Option<&str>,
    env: BTreeMap<String, String>,
    run_id: Option<String>,
) -> Result<(Value, i32), Failure> {
    let specs = select(surface, filter);
    if specs.is_empty() {
        let known: Vec<&str> = registry().iter().map(|spec| spec.title).collect();
        return Err(fail(
            "specs.select",
            match filter {
                Some(want) => format!(
                    "no {surface} journey matches {want} (one of {})",
                    known.join(", ")
                ),
                None => format!("no journey is registered for surface {surface}"),
            },
        ));
    }
    fs::create_dir_all(artifacts).map_err(|error| {
        fail(
            "specs.artifacts",
            format!("{}: {error}", artifacts.display()),
        )
    })?;

    let mut capture_errors: Vec<String> = Vec::new();
    let mut rows: Vec<Value> = Vec::new();
    for spec in &specs {
        let context = Context {
            harness: harness.to_path_buf(),
            artifacts: artifacts.to_path_buf(),
            title: spec.title.to_string(),
            env: env.clone(),
            media: Mutex::new(Vec::new()),
        };
        let started_at = SystemTime::now();
        let started = Instant::now();
        let outcome = catch_unwind(AssertUnwindSafe(|| (spec.run)(&context)));
        let duration = started.elapsed();
        let error = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(reason)) => Some(reason),
            Err(panic) => Some(format!("journey panicked: {}", panic_text(panic))),
        };
        let media = match validate_media(artifacts, &context.declared_media()) {
            Ok(media) => media,
            Err(reason) => {
                capture_errors.push(format!("{}: {reason}", spec.title));
                Vec::new()
            }
        };
        rows.push(json!({
            "title": spec.title,
            "passed": error.is_none(),
            "status": if error.is_none() { "passed" } else { "failed" },
            "flaky": false,
            "attempts": 1,
            "duration": duration.as_millis(),
            "startedAt": iso_timestamp(started_at),
            "completedAt": at_iso(started_at, duration),
            "error": error.as_deref().map(clip_row_error).map(Value::from).unwrap_or(Value::Null),
            "media": media,
        }));
    }

    let passed = rows.iter().filter(|row| row["status"] == "passed").count();
    let report = json!({
        "probierz": { "runId": run_id, "captureErrors": capture_errors },
        "total": rows.len(),
        "passed": passed,
        "failed": rows.len() - passed,
        "flaky": 0,
        "skipped": 0,
        "tests": rows,
    });
    write_report(report_path, &report)?;
    let code = if passed == rows.len() { 0 } else { 1 };
    Ok((report, code))
}

fn panic_text(panic: Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    if let Some(text) = panic.downcast_ref::<String>() {
        return text.clone();
    }
    "unknown panic".to_string()
}

fn write_report(path: &Path, report: &Value) -> Result<(), Failure> {
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
