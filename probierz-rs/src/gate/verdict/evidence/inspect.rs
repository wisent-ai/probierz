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
    // An identity counts only when the run states it as non-empty text.
    let stated = |value: &Value, name: &str| string_property(value, name).filter(|text| !text.is_empty());
    let harness_sha = stated(&run.harness, "sha256");
    let harness_complete = harness_sha.is_some()
        && stated(&run.harness, "gitSha").is_some()
        && stated(&run.harness, "worktreeSha256").is_some();
    match &harness_sha {
        Some(sha) if harness_complete => {
            if sha != &args.expected_harness_sha {
                errors.push(format!(
                    "{}: harness source {sha} does not match {}",
                    run.run_id, args.expected_harness_sha
                ));
            }
        }
        _ => errors.push(format!(
            "{}: complete harness source identity is missing",
            run.run_id
        )),
    }
    if stated(&run.build, "sha256").is_none() {
        errors.push(format!("{}: exact build hash is missing", run.run_id));
    }
    if evidence_rank(evidence_level(run)) < Some(required_rank) {
        errors.push(format!(
            "{}: {} is below {}",
            run.run_id,
            evidence_level(run),
            minimum_evidence
        ));
    }
    let source_sha = stated(&run.source, "sha256");
    // Every repository the source names states its commit and worktree; a
    // source that names no repositories is not a complete identity.
    let repositories_complete = property(&run.source, "repositories")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().all(|repository| {
                stated(repository, "gitSha").is_some() && stated(repository, "worktreeSha256").is_some()
            })
        });
    if source_sha.is_none() || !repositories_complete {
        errors.push(format!(
            "{}: complete app source identity is missing",
            run.run_id
        ));
    }
    if let Some(expected_source) = expected_source.filter(|value| !value.is_empty()) {
        match source_sha.as_deref() {
            Some(sha) if sha == expected_source => {}
            Some(sha) => errors.push(format!(
                "{}: app source {sha} does not match {expected_source}",
                run.run_id
            )),
            None => errors.push(format!(
                "{}: app source is missing, so it cannot match {expected_source}",
                run.run_id
            )),
        }
    }
    // At the E3 rank every artifact states its hash.
    if Some(required_rank) >= evidence_rank("E3")
        && (run.artifacts.is_empty()
            || run.artifacts.iter().any(|artifact| stated(artifact, "sha256").is_none()))
    {
        errors.push(format!("{}: E3 artifact hashes are incomplete", run.run_id));
    }
    if !is_true(property(&run.protection, "plaintextRemoved")) {
        // A run's manifest lies in its run directory, so it has one.
        let Some(artifact_root) = run.manifest_path.parent() else {
            errors.push(format!(
                "{}: run manifest {} lies in no directory, so its artifacts cannot be found",
                run.run_id,
                run.manifest_path.display()
            ));
            return;
        };
        let normalized_root = match normalize_absolute(artifact_root) {
            Ok(root) => root,
            Err(cause) => {
                errors.push(format!("{}: artifacts cannot be located: {cause}", run.run_id));
                return;
            }
        };
        for artifact in &run.artifacts {
            let Some(relative) = stated(artifact, "file") else {
                errors.push(format!("{}: an artifact names no file: {artifact}", run.run_id));
                continue;
            };
            let file = match normalize_absolute(&artifact_root.join(&relative)) {
                Ok(file) => file,
                Err(cause) => {
                    errors.push(format!("{}: artifact {relative} cannot be located: {cause}", run.run_id));
                    continue;
                }
            };
            if file != normalized_root && !file.starts_with(&normalized_root) {
                errors.push(format!(
                    "{}: artifact path escapes its run: {relative}",
                    run.run_id
                ));
                continue;
            }
            match crate::evidence::sha256_file(&file) {
                Ok(actual) if Some(actual.as_str()) != string_property(artifact, "sha256").as_deref() => {
                    errors.push(format!("{}: artifact hash mismatch: {relative}", run.run_id))
                }
                Ok(_) => {}
                Err(error) => errors.push(format!(
                    "{}: artifact is missing or cannot be hashed: {relative} ({error})",
                    run.run_id
                )),
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
    if require_protected || is_true(property(&run.protection, "plaintextRemoved")) {
        let protected_file = stated(&run.protection, "file");
        let bundle = match protected_file {
            Some(file) if is_true(property(&run.protection, "plaintextRemoved")) => Some(file),
            _ => None,
        };
        if let Some(file) = bundle {
            match crate::evidence::sha256_file(Path::new(&file)) {
                Ok(hash) => {
                    bundle_hashes.insert(run.run_id.clone(), hash.clone());
                    if string_property(&run.protection, "sha256").as_deref() != Some(hash.as_str())
                    {
                        errors.push(format!(
                            "{}: encrypted bundle hash does not match its manifest",
                            run.run_id
                        ));
                    }
                }
                Err(error) => errors.push(format!(
                    "{}: encrypted-at-rest artifact bundle is missing or cannot be hashed: {error}",
                    run.run_id
                )),
            }
        } else {
            errors.push(format!(
                "{}: encrypted-at-rest artifact bundle is missing",
                run.run_id
            ));
        }
    }
    if require_secret_scan
        && !is_true(
            property(&run.protection, "secretScan").and_then(|scan| property(scan, "passed")),
        )
    {
        errors.push(format!(
            "{}: passing pre-upload secret scan is missing",
            run.run_id
        ));
    }
}
