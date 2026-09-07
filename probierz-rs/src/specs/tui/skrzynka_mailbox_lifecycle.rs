use crate::specs::{self, tui::common};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

pub fn run(context: &specs::Context) -> Result<(), String> {
    let repo = PathBuf::from(context.optional("SKRZYNKA_REPO").unwrap_or_else(|| {
        "/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent/skrzynka".into()
    }));
    let owned = repo.join("tests/mailboxes/mailbox-lifecycle.probierz.spec.mjs");
    if !owned.exists() {
        return Err(format!("skrzynka no longer carries {}; this journey has no test to run, which is a failure and not a pass",owned.display()));
    }
    let args = vec![
        "--test".into(),
        "--test-reporter=tap".into(),
        owned.to_string_lossy().into_owned(),
    ];
    let run = common::run(
        "node",
        &args,
        Some(&repo),
        &BTreeMap::new(),
        &[],
        None,
        Duration::from_secs(900),
    )?;
    let output = run.combined();
    if !run.status.success() {
        return Err(format!(
            "the repository's own mailbox-lifecycle spec failed with {}\n{output}",
            run.code()
                .map_or_else(|| "signal".into(), |c| c.to_string())
        ));
    }
    let mut planned = None;
    let mut results = Vec::new();
    let plan_re = regex::Regex::new(r"^1\.\.(\d+)$").unwrap();
    let result_re = regex::Regex::new(r"^(not ok|ok)\s+\d+\b").unwrap();
    for line in run.stdout.lines() {
        if let Some(c) = plan_re.captures(line) {
            planned = c[1].parse::<usize>().ok();
            continue;
        }
        if let Some(c) = result_re.captures(line) {
            let upper = line.to_uppercase();
            let directive = if upper.contains("# SKIP") {
                "SKIP"
            } else if upper.contains("# TODO") {
                "TODO"
            } else {
                ""
            };
            results.push((c.get(1).unwrap().as_str() == "ok", directive));
        }
    }
    let planned=planned.ok_or_else(||format!("Probierz could not read the repository spec's TAP result: top-level TAP plan (`1..N`) was absent\nTAP stdout:\n{}\nstderr:\n{}",run.stdout,run.stderr))?;
    if results.len() != planned {
        return Err(format!("Probierz could not read the repository spec's TAP result: top-level TAP plan declared {planned} tests but reported {} results\nTAP stdout:\n{}\nstderr:\n{}",results.len(),run.stdout,run.stderr));
    }
    let summary = (
        planned,
        results.iter().filter(|(ok, d)| *ok && d.is_empty()).count(),
        results
            .iter()
            .filter(|(ok, d)| !*ok && *d != "TODO")
            .count(),
        results.iter().filter(|(_, d)| *d == "SKIP").count(),
        results.iter().filter(|(_, d)| *d == "TODO").count(),
    );
    if summary != (1, 1, 0, 0, 0) {
        return Err(format!("the repository's TAP result was not one completed passing test\nTAP stdout:\n{}\nstderr:\n{}",run.stdout,run.stderr));
    }
    Ok(())
}
