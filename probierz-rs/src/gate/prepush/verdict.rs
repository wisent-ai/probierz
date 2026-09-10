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
    let resolved_head = head
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| git(repo, &["rev-parse", "HEAD"]));
    let resolved_base =
        if let Some(base) = base.filter(|base| !base.is_empty() && *base != ZERO_SHA) {
            git(repo, &["rev-parse", base])
        } else if let Some(head) = &resolved_head {
            git(repo, &["merge-base", head, "origin/main"])
        } else {
            None
        };
    let Some(resolved_base) = resolved_base else {
        return Ok(object([
            ("ok", Value::Bool(false)),
            ("appId", Value::String(resolved_app)),
            (
                "reason",
                Value::String(
                    "cannot resolve a merge base with origin/main; fetch first or pass --base"
                        .to_string(),
                ),
            ),
        ]));
    };
    let resolved_head = resolved_head.unwrap_or_default();
    let range = format!("{resolved_base}..{resolved_head}");
    let files: Vec<PathBuf> = git_lines(repo, &["diff", "--name-only", &range])
        .into_iter()
        .map(|file| repo.join(file))
        .collect();
    let journeys = affected_journeys(&app, &files);
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
        let status = command.status()?;
        if !status.success() {
            return Ok(object([
                ("ok", Value::Bool(false)),
                ("appId", Value::String(resolved_app)),
                ("base", Value::String(resolved_base)),
                ("head", Value::String(resolved_head)),
                ("affectedJourneys", strings(&journeys)),
                (
                    "reason",
                    Value::String(format!(
                        "probierz ci failed (exit {})",
                        status
                            .code()
                            .map(|code| code.to_string())
                            .unwrap_or_else(|| "null".to_string())
                    )),
                ),
            ]));
        }
    }
    let mut history = all_runs(harness, &resolved_app)
        .map_err(|detail| Failure::config("gate.prepush", detail))?;
    history.sort_by(|left, right| {
        js_display(Some(&right.started_at)).cmp(&js_display(Some(&left.started_at)))
    });
    history.truncate(1000);
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
    let identity = crate::evidence::app_source_identity_value(harness, &resolved_app)?;
    let gate_args = GateArgs {
        app_id: resolved_app.clone(),
        mode: "pull-request".to_string(),
        expected_harness_sha: string_property(
            property(&identity, "harness").unwrap_or(&Value::Null),
            "sha256",
        )
        .unwrap_or_default(),
        expected_source_sha: string_property(
            property(&identity, "app").unwrap_or(&Value::Null),
            "sha256",
        ),
        runs: Some(run_ids.join(",")),
        release: None,
        receipt: None,
        public_key: None,
        fingerprint: None,
    };
    let evaluation = evaluate_value(harness, &gate_args)?;
    let verdict = property(&evaluation, "verdict")
        .cloned()
        .unwrap_or(Value::Null);
    let ok = truthy(property(&verdict, "passed"));
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

pub(crate) fn parse_hook_refs(text: &str) -> Option<(String, String)> {
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        if matches!(parts[2], "refs/heads/main" | "refs/heads/master") {
            let base = if parts[3] == ZERO_SHA {
                String::new()
            } else {
                parts[3].to_string()
            };
            return Some((base, parts[1].to_string()));
        }
    }
    None
}

