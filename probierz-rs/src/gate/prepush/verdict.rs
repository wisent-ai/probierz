use crate::gate::*;
pub(crate) fn prepush_value(
    harness: &Path,
    repo: &Path,
    app_id: Option<&str>,
    base: Option<&str>,
    head: Option<&str>,
    run_ci: bool,
    ci_args: &[String],
) -> Result<Value, Failure> {
    let resolved_app = match app_id.map(str::to_string).or(infer_app_id(harness, repo)?) {
        Some(app) => app,
        None => {
            return Ok(object([
                ("ok", Value::Bool(false)),
                (
                    "reason",
                    Value::String(format!(
                        "no probierz app manifest matches {}",
                        repo.display()
                    )),
                ),
            ]))
        }
    };
    let app = manifest::load(harness, &resolved_app)?;
    let resolved_head = match head.filter(|value| !value.is_empty()) {
        Some(head) => Ok(head.to_string()),
        None => revision(repo, &["rev-parse", "HEAD"])
            .map_err(|cause| format!("cannot resolve HEAD: {cause}")),
    };
    // Each ref that does not resolve is the verdict's reason with git's own
    // words: read as absent, the gate said "cannot resolve" and not why.
    let stated_base = base.filter(|base| !base.is_empty() && *base != ZERO_SHA);
    let resolved_base = match (stated_base, &resolved_head) {
        (Some(base), _) => revision(repo, &["rev-parse", base])
            .map_err(|cause| format!("cannot resolve --base {base}: {cause}")),
        (None, Ok(head)) => revision(
            repo,
            &["merge-base", head, crate::status::TRACKED_UPSTREAM],
        )
        .map_err(|cause| {
            format!(
                "cannot resolve a merge base with the branch this checkout tracks; set an upstream or pass --base: {cause}"
            )
        }),
        (None, Err(cause)) => Err(cause.clone()),
    };
    let resolved_base = match resolved_base {
        Ok(base) => base,
        Err(reason) => {
            return Ok(object([
                ("ok", Value::Bool(false)),
                ("appId", Value::String(resolved_app)),
                ("reason", Value::String(reason)),
            ]))
        }
    };
    // A head that does not resolve, or a diff git cannot produce, is the
    // verdict's reason: read as no files, the gate passed with no journeys run.
    let unread = |reason: String| -> Result<Value, Failure> {
        Ok(object([
            ("ok", Value::Bool(false)),
            ("appId", Value::String(resolved_app.clone())),
            ("reason", Value::String(reason)),
        ]))
    };
    let resolved_head = match resolved_head {
        Ok(head) => head,
        Err(reason) => return unread(reason),
    };
    let range = format!("{resolved_base}..{resolved_head}");
    let files: Vec<PathBuf> = match git_lines(repo, &["diff", "--name-only", &range]) {
        Ok(lines) => lines.into_iter().map(|file| repo.join(file)).collect(),
        Err(reason) => return unread(reason),
    };
    let journeys = affected_journeys(&app, &files)?;
    if journeys.is_empty() {
        return Ok(object([
            ("ok", Value::Bool(true)),
            ("appId", Value::String(resolved_app)),
            ("base", Value::String(resolved_base)),
            ("head", Value::String(resolved_head)),
            ("affectedJourneys", Value::Array(Vec::new())),
            (
                "verdict",
                object([
                    ("passed", Value::Bool(true)),
                    ("errors", Value::Array(Vec::new())),
                ]),
            ),
            ("note", Value::String("no affected journeys".to_string())),
        ]));
    }
    if run_ci {
        let executable = std::env::current_exe()?;
        let mut command = ProcessCommand::new(executable);
        command
            .arg("--harness")
            .arg(harness)
            .arg("ci")
            .arg(&resolved_base)
            .arg("--app")
            .arg(&resolved_app)
            .args(ci_args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let status = stado_wait::status(&mut command)?;
        if !status.success() {
            return Ok(object([
                ("ok", Value::Bool(false)),
                ("appId", Value::String(resolved_app)),
                ("base", Value::String(resolved_base)),
                ("head", Value::String(resolved_head)),
                ("affectedJourneys", strings(&journeys)),
                (
                    "reason",
                    Value::String(format!("probierz ci {}", crate::failure::ended(&status))),
                ),
            ]));
        }
    }
    let mut history = all_runs(harness, &resolved_app)
        .map_err(|detail| Failure::config("gate.prepush", detail))?;
    // Newest first by the start each run states (a run stating none last),
    // and every run kept: the gate looks for each journey's newest passing
    // run, which no cap nobody stated may cut off.
    history.sort_by(|left, right| right.started_at.as_str().cmp(&left.started_at.as_str()));
    let mut run_ids = Vec::new();
    for journey in &journeys {
        if let Some(run) = history
            .iter()
            .find(|run| run.journeys.contains(journey) && run.status == "passed")
        {
            if !run_ids.contains(&run.run_id) {
                run_ids.push(run.run_id.clone());
            }
        }
    }
    if run_ids.is_empty() {
        return Ok(object([
            ("ok", Value::Bool(false)),
            ("appId", Value::String(resolved_app)),
            ("base", Value::String(resolved_base)),
            ("head", Value::String(resolved_head)),
            ("affectedJourneys", strings(&journeys)),
            ("reason", Value::String("no passing runs recorded for the affected journeys; run `probierz ci <base>` (or re-run with --ci) before pushing".to_string())),
        ]));
    }
    let identity = crate::authoring::app_source_identity(harness, &resolved_app, None)?;
    let harness_sha = property(&identity, "harness")
        .and_then(|harness| string_property(harness, "sha256"))
        .ok_or_else(|| {
            Failure::config(
                "gate.prepush",
                format!(
                    "the source identity of {resolved_app} states no harness sha256: {identity}"
                ),
            )
        })?;
    let gate_args = GateArgs {
        app_id: resolved_app.clone(),
        mode: "pull-request".to_string(),
        expected_harness_sha: harness_sha,
        expected_source_sha: property(&identity, "app")
            .and_then(|app| string_property(app, "sha256")),
        runs: Some(run_ids.join(",")),
        release: None,
        receipt: None,
        public_key: None,
        fingerprint: None,
    };
    let evaluation = evaluate_value(harness, &gate_args)?;
    let verdict = property(&evaluation, "verdict").cloned().ok_or_else(|| {
        Failure::config(
            "gate.prepush",
            format!("the gate evaluation of {resolved_app} states no verdict: {evaluation}"),
        )
    })?;
    let ok = is_true(property(&verdict, "passed"));
    Ok(object([
        ("ok", Value::Bool(ok)),
        ("appId", Value::String(resolved_app)),
        ("base", Value::String(resolved_base)),
        ("head", Value::String(resolved_head)),
        ("affectedJourneys", strings(&journeys)),
        ("runIds", strings(&run_ids)),
        ("verdict", verdict),
    ]))
}

/// The base and head of the push to the gated branch, read from the lines
/// git hands a pre-push hook (`<local ref> <local sha> <remote ref> <remote
/// sha>`, githooks(5)); a remote sha of zeros is a new branch, whose base is
/// left for the merge base.
pub(crate) fn parse_hook_refs(text: &str) -> Option<(String, String)> {
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [_, local_sha, remote_ref, remote_sha] = fields[..] else {
            continue;
        };
        if matches!(remote_ref, "refs/heads/main" | "refs/heads/master") {
            let base = if remote_sha == ZERO_SHA {
                String::new()
            } else {
                remote_sha.to_string()
            };
            return Some((base, local_sha.to_string()));
        }
    }
    None
}
