use crate::gate::*;

/// What each run has to prove on its own before a verdict may read it: that
/// it passed, that it names the exact harness and app source the gate was
/// given, that its evidence reaches the declared level, and that its
/// artifacts are protected and scanned where the policy says so. Every
/// shortfall is pushed onto `errors`; the encrypted bundle digests the
/// receipt is later compared against are collected into `bundle_hashes`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn inspect_runs(
    args: &GateArgs,
    runs: &[Run],
    minimum_evidence: &str,
    required_rank: i32,
    expected_source: Option<&str>,
    require_protected: bool,
    require_secret_scan: bool,
    bundle_hashes: &mut HashMap<String, String>,
    errors: &mut Vec<String>,
) {
    for run in runs {
        inspect_run(
            args,
            run,
            minimum_evidence,
            required_rank,
            expected_source,
            require_protected,
            require_secret_scan,
            bundle_hashes,
            errors,
        );
    }
}

/// One run, read on its own terms.
#[allow(clippy::too_many_arguments)]
fn inspect_run(
    args: &GateArgs,
    run: &Run,
    minimum_evidence: &str,
    required_rank: i32,
    expected_source: Option<&str>,
    require_protected: bool,
    require_secret_scan: bool,
    bundle_hashes: &mut HashMap<String, String>,
    errors: &mut Vec<String>,
) {
        if run.status != "passed" {
            errors.push(format!("{}: status is {}", run.run_id, run.status));
        }
        let harness_sha = string_property(&run.harness, "sha256");
        let harness_git = string_property(&run.harness, "gitSha");
        let harness_worktree = string_property(&run.harness, "worktreeSha256");
        if harness_sha.as_deref().unwrap_or("").is_empty()
            || harness_git.as_deref().unwrap_or("").is_empty()
            || harness_worktree.as_deref().unwrap_or("").is_empty()
        {
            errors.push(format!(
                "{}: complete harness source identity is missing",
                run.run_id
            ));
        } else if harness_sha.as_deref() != Some(args.expected_harness_sha.as_str()) {
            errors.push(format!(
                "{}: harness source {} does not match {}",
                run.run_id,
                harness_sha.unwrap_or_default(),
                args.expected_harness_sha
            ));
        }
        if string_property(&run.build, "sha256")
            .as_deref()
            .unwrap_or("")
            .is_empty()
        {
            errors.push(format!("{}: exact build hash is missing", run.run_id));
        }
        if evidence_rank(evidence_level(run)).unwrap_or(-1) < required_rank {
            errors.push(format!(
                "{}: {} is below {}",
                run.run_id,
                evidence_level(run),
                minimum_evidence
            ));
        }
        let source_sha = string_property(&run.source, "sha256");
        let repositories = property(&run.source, "repositories").and_then(Value::as_array);
        let repositories_complete = repositories
            .map(|items| {
                items.iter().all(|repository| {
                    !string_property(repository, "gitSha")
                        .as_deref()
                        .unwrap_or("")
                        .is_empty()
                        && !string_property(repository, "worktreeSha256")
                            .as_deref()
                            .unwrap_or("")
                            .is_empty()
                })
            })
            .unwrap_or(false);
        if source_sha.as_deref().unwrap_or("").is_empty() || !repositories_complete {
            errors.push(format!(
                "{}: complete app source identity is missing",
                run.run_id
            ));
        }
        if let Some(expected_source) = expected_source.filter(|value| !value.is_empty()) {
            if source_sha.as_deref() != Some(expected_source) {
                errors.push(format!(
                    "{}: app source {} does not match {expected_source}",
                    run.run_id,
                    source_sha.unwrap_or_else(|| "missing".to_string())
                ));
            }
        }
        if required_rank >= 3
            && (run.artifacts.is_empty()
                || run.artifacts.iter().any(|artifact| {
                    string_property(artifact, "sha256")
                        .as_deref()
                        .unwrap_or("")
                        .is_empty()
                }))
        {
            errors.push(format!("{}: E3 artifact hashes are incomplete", run.run_id));
        }
        if !truthy(property(&run.protection, "plaintextRemoved")) {
            let artifact_root = run.manifest_path.parent().unwrap_or_else(|| Path::new("."));
            for artifact in &run.artifacts {
                let relative = string_property(artifact, "file").unwrap_or_default();
                let file = normalize_absolute(&artifact_root.join(&relative));
                let normalized_root = normalize_absolute(artifact_root);
                if file != normalized_root && !file.starts_with(&normalized_root) {
                    errors.push(format!(
                        "{}: artifact path escapes its run: {relative}",
                        run.run_id
                    ));
                } else if !file.exists() {
                    errors.push(format!("{}: artifact is missing: {relative}", run.run_id));
                } else {
                    match sha256_file(&file) {
                        Ok(actual)
                            if Some(actual.as_str())
                                != string_property(artifact, "sha256").as_deref() =>
                        {
                            errors.push(format!(
                                "{}: artifact hash mismatch: {relative}",
                                run.run_id
                            ))
                        }
                        Ok(_) => {}
                        Err(error) => errors.push(format!(
                            "{}: artifact cannot be hashed: {relative} ({error})",
                            run.run_id
                        )),
                    }
                }
            }
        }
        if run.kind != args.mode {
            errors.push(format!(
                "{}: run kind {} is not {}",
                run.run_id, run.kind, args.mode
            ));
        }
        if let Some(release) = args.release.as_deref().filter(|value| !value.is_empty()) {
            if args.mode == "release"
                && string_property(&run.conditions, "PROBIERZ_RELEASE").as_deref() != Some(release)
            {
                errors.push(format!(
                    "{}: release condition does not match {release}",
                    run.run_id
                ));
            }
        }
        if require_protected || truthy(property(&run.protection, "plaintextRemoved")) {
            let protected_file = string_property(&run.protection, "file");
            if !truthy(property(&run.protection, "plaintextRemoved"))
                || protected_file.as_deref().unwrap_or("").is_empty()
                || !protected_file
                    .as_deref()
                    .map(Path::new)
                    .map(Path::exists)
                    .unwrap_or(false)
            {
                errors.push(format!(
                    "{}: encrypted-at-rest artifact bundle is missing",
                    run.run_id
                ));
            } else if let Some(file) = protected_file {
                match sha256_file(Path::new(&file)) {
                    Ok(hash) => {
                        bundle_hashes.insert(run.run_id.clone(), hash.clone());
                        if string_property(&run.protection, "sha256").as_deref()
                            != Some(hash.as_str())
                        {
                            errors.push(format!(
                                "{}: encrypted bundle hash does not match its manifest",
                                run.run_id
                            ));
                        }
                    }
                    Err(error) => errors.push(format!(
                        "{}: encrypted bundle cannot be hashed: {error}",
                        run.run_id
                    )),
                }
            }
        }
        if require_secret_scan
            && !truthy(
                property(&run.protection, "secretScan").and_then(|scan| property(scan, "passed")),
            )
        {
            errors.push(format!(
                "{}: passing pre-upload secret scan is missing",
                run.run_id
            ));
        }
}
