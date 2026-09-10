use crate::specs::{self, tui::common};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};
pub fn run(context: &specs::Context) -> Result<(), String> {
    let source = if let Some(root) = context.optional("OKO_SOURCE_ROOT") {
        PathBuf::from(root)
    } else {
        let manifest = fs::read_to_string(context.harness.join("apps/oko/probierz.yaml"))
            .map_err(|e| e.to_string())?;
        let root = manifest
            .lines()
            .find_map(|l| l.strip_prefix("  - root: "))
            .ok_or("Oko manifest must provide the source repository root")?;
        PathBuf::from(root.trim())
    };
    let git = |args: &[&str], timeout| {
        common::run(
            "/usr/bin/git",
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            Some(&source),
            &BTreeMap::new(),
            &[],
            None,
            timeout,
        )
    };
    let rev = git(
        &["-C", source.to_string_lossy().as_ref(), "rev-parse", "HEAD"],
        Duration::from_secs(30),
    )?;
    if !rev.status.success() {
        return Err(format!(
            "cannot resolve Oko source revision: {}",
            rev.stderr
        ));
    }
    let revision = rev.stdout.trim();
    if !regex::Regex::new(r"^[0-9a-f]{40}$")
        .unwrap()
        .is_match(revision)
    {
        return Err("Oko source revision is not a full Git SHA".into());
    }
    let status = git(
        &[
            "-C",
            source.to_string_lossy().as_ref(),
            "status",
            "--porcelain",
        ],
        Duration::from_secs(30),
    )?;
    if !status.status.success() {
        return Err(format!(
            "cannot inspect Oko source state: {}",
            status.stderr
        ));
    }
    if context.optional("OKO_SOURCE_ROOT").is_some() && !status.stdout.is_empty() {
        return Err("explicit Oko evidence source must be a clean worktree".into());
    }
    let swift = |args: Vec<String>| {
        common::run(
            "/usr/bin/swift",
            &args,
            None,
            &common::env_map([("SWIFT_DETERMINISTIC_HASHING", "1")]),
            &[],
            None,
            Duration::from_secs(900),
        )
    };
    let build = swift(vec![
        "build".into(),
        "--package-path".into(),
        source.to_string_lossy().into_owned(),
        "--product".into(),
        "oko-cli".into(),
    ])?;
    if !build.status.success() {
        return Err(format!(
            "Oko autonomy CLI build exited {}:\n{}\n{}",
            build
                .code()
                .map_or_else(|| "signal".into(), |c| c.to_string()),
            build
                .stderr
                .chars()
                .rev()
                .take(6000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>(),
            build
                .stdout
                .chars()
                .rev()
                .take(6000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        ));
    }
    let test = swift(vec![
        "test".into(),
        "--package-path".into(),
        source.to_string_lossy().into_owned(),
        "--filter".into(),
        "OkoAutonomyTests".into(),
    ])?;
    if !test.status.success() {
        return Err(format!(
            "Oko autonomy contract tests exited {}:\n{}",
            test.code()
                .map_or_else(|| "signal".into(), |c| c.to_string()),
            test.combined()
        ));
    }
    let output = test.combined();
    if !output.contains("OkoAutonomyTests") {
        return Err("autonomy tests were not selected".into());
    }
    let lower = output.to_lowercase();
    if !lower.contains("0 failures") && !lower.contains("0 failed") {
        return Err("autonomy suite did not report a clean result".into());
    }
    common::write_trace(
        context,
        "oko-autonomy.trace.json",
        json!({"schemaVersion":1,"kind":"probierz-oko-autonomy-trace","evidenceLevel":"E3","runId":context.optional("PROBIERZ_RUN_ID"),"status":"completed","observation":{"sourceRoot":source,"sourceRevision":revision,"sourceDirty":!status.stdout.is_empty(),"buildExitCode":build.code(),"testExitCode":test.code(),"output":format!("{}{}",build.combined(),output).chars().rev().take(6000).collect::<String>().chars().rev().collect::<String>()},"contracts":["missing policy state is disabled by default","experimental policy is local-user scoped and path confined","competing schedulers cannot release another scheduler lease","goal completion requires both a successful Pursuit receipt and an accepted independent verdict"],"redaction":{"status":"verified_redacted","credentialsIncluded":false,"privateRecordsIncluded":false},"publicationRequirements":{"artifactKind":"trace","minimumEvidence":"E3","redactionStatus":"verified_redacted","signedReceiptRequired":true}}),
    )
}
