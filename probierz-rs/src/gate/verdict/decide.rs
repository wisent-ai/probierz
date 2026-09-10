use crate::gate::*;
pub(crate) fn mode_policy<'a>(app: &'a manifest::Manifest, mode: &str) -> Option<&'a Yaml> {
    yaml_get(
        &app.document,
        if mode == "release" {
            "releasePolicy"
        } else {
            "pullRequestPolicy"
        },
    )
}

pub(crate) fn evaluate_value(harness: &Path, args: &GateArgs) -> Result<Value, Failure> {
    if args.app_id.is_empty() || !matches!(args.mode.as_str(), "pull-request" | "release") {
        return Err(Failure::invalid(
            "gate.evaluate",
            "gate needs an app ID and pull-request or release mode",
        ));
    }
    let app = manifest::load(harness, &args.app_id)?;
    let empty_policy = Yaml::Mapping(Default::default());
    let policy = mode_policy(&app, &args.mode).unwrap_or(&empty_policy);
    let minimum_evidence =
        yaml_string(yaml_get(policy, "minimumEvidence")).unwrap_or_else(|| "E3".to_string());
    let required_rank = evidence_rank(&minimum_evidence).ok_or_else(|| {
        Failure::config(
            "gate.evaluate",
            format!("unsupported gate evidence level: {minimum_evidence}"),
        )
    })?;
    let required_targets = yaml_strings(yaml_get(policy, "requiredTargets"));
    let required_journeys = yaml_strings(yaml_get(policy, "requiredJourneys"));
    let matrix_profile = yaml_string(yaml_get(policy, "requiredMatrixProfile"));
    let require_protected = yaml_bool(yaml_get(policy, "requireProtectedArtifacts"));
    let require_secret_scan = yaml_bool(yaml_get(policy, "requireSecretScan"));
    let run_ids: Vec<String> = args
        .runs
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    let mut errors = Vec::new();
    if args.expected_harness_sha.is_empty() {
        errors.push("expected harness source SHA-256 is required".to_string());
    }
    if args.expected_source_sha.as_deref().unwrap_or("").is_empty() {
        errors.push("expected app source SHA-256 is required".to_string());
    }
    if run_ids.is_empty() {
        errors.push("at least one run ID is required".to_string());
    }
    if run_ids.iter().collect::<BTreeSet<_>>().len() != run_ids.len() {
        errors.push("run IDs must be unique".to_string());
    }
    let mut seen = BTreeSet::new();
    let mut runs = Vec::new();
    for run_id in &run_ids {
        if !seen.insert(run_id.clone()) {
            continue;
        }
        match get_run(harness, &args.app_id, run_id) {
            Ok(run) => runs.push(run),
            Err(error) => errors.push(error),
        }
    }
    let expected_source = args.expected_source_sha.as_deref();
    let mut bundle_hashes: HashMap<String, String> = HashMap::new();
    inspect_runs(
        args,
        &runs,
        &minimum_evidence,
        required_rank,
        expected_source,
        require_protected,
        require_secret_scan,
        &mut bundle_hashes,
        &mut errors,
    );
    let source_hashes: BTreeSet<String> = runs
        .iter()
        .filter_map(|run| string_property(&run.source, "sha256"))
        .filter(|value| !value.is_empty())
        .collect();
    if !runs.is_empty() && source_hashes.len() != 1 {
        errors.push(format!(
            "runs do not identify one exact app source ({} source hashes)",
            source_hashes.len()
        ));
    }
    let mut builds = Map::new();
    for run in &runs {
        let Some(hash) = string_property(&run.build, "sha256").filter(|value| !value.is_empty())
        else {
            continue;
        };
        if let Some(existing) = builds.get(&run.target).and_then(Value::as_str) {
            if existing != hash {
                errors.push(format!(
                    "{}: runs do not identify one exact build",
                    run.target
                ));
            }
        } else {
            builds.insert(run.target.clone(), Value::String(hash));
        }
    }
    for target in &required_targets {
        if !runs.iter().any(|run| run.target == *target) {
            errors.push(format!("required target is missing: {target}"));
        }
    }
    for journey in &required_journeys {
        if !runs.iter().any(|run| run.journeys.contains(journey)) {
            errors.push(format!("required journey is missing: {journey}"));
        }
    }
    let matrix = if let Some(profile) = matrix_profile.as_deref() {
        let coverage = matrix_coverage(&app, profile, &runs)
            .map_err(|detail| Failure::config("gate.matrix", detail))?;
        let missing = property(&coverage, "missing")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        let extra = property(&coverage, "extraRunIds")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        if missing > 0 {
            errors.push(format!("{missing} required matrix cell(s) are missing"));
        }
        if extra > 0 {
            errors.push(format!("{extra} run(s) are outside the required matrix"));
        }
        coverage
    } else {
        Value::Null
    };
    let receipt = release_receipt(
        args,
        &app,
        &runs,
        &run_ids,
        &builds,
        &bundle_hashes,
        expected_source,
        require_protected,
        &mut errors,
    )?;
    let harness_matches = !runs.is_empty()
        && runs.iter().all(|run| {
            string_property(&run.harness, "sha256").as_deref()
                == Some(args.expected_harness_sha.as_str())
        });
    let source_sha = if source_hashes.len() == 1 {
        source_hashes
            .iter()
            .next()
            .cloned()
            .map(Value::String)
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let mut levels = Map::new();
    for run in &runs {
        levels.insert(
            run.run_id.clone(),
            Value::String(evidence_level(run).to_string()),
        );
    }
    let result = object([
        ("schemaVersion", Value::from(2)),
        ("appId", Value::String(args.app_id.clone())),
        ("mode", Value::String(args.mode.clone())),
        (
            "release",
            args.release
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        ),
        (
            "expectedHarnessSha",
            if args.expected_harness_sha.is_empty() {
                Value::Null
            } else {
                Value::String(args.expected_harness_sha.clone())
            },
        ),
        (
            "expectedSourceSha",
            expected_source
                .filter(|value| !value.is_empty())
                .map(|value| Value::String(value.to_string()))
                .unwrap_or(Value::Null),
        ),
        (
            "policy",
            object([
                ("minimumEvidence", Value::String(minimum_evidence)),
                ("requiredTargets", strings(&required_targets)),
                ("requiredJourneys", strings(&required_journeys)),
                (
                    "requiredMatrixProfile",
                    matrix_profile.map(Value::String).unwrap_or(Value::Null),
                ),
                ("requireProtectedArtifacts", Value::Bool(require_protected)),
                ("requireSecretScan", Value::Bool(require_secret_scan)),
            ]),
        ),
        (
            "verdict",
            object([
                ("passed", Value::Bool(errors.is_empty())),
                (
                    "errors",
                    Value::Array(errors.iter().cloned().map(Value::String).collect()),
                ),
            ]),
        ),
        (
            "evidence",
            object([
                (
                    "runIds",
                    Value::Array(
                        runs.iter()
                            .map(|run| Value::String(run.run_id.clone()))
                            .collect(),
                    ),
                ),
                ("builds", Value::Object(builds)),
                (
                    "harnessSha256",
                    if harness_matches {
                        Value::String(args.expected_harness_sha.clone())
                    } else {
                        Value::Null
                    },
                ),
                ("sourceSha256", source_sha),
                ("levels", Value::Object(levels)),
                ("matrix", matrix),
                ("receipt", receipt),
            ]),
        ),
    ]);
    audit_access(
        harness,
        "gate.evaluate",
        if errors.is_empty() {
            "allowed"
        } else {
            "denied"
        },
        Some(&args.app_id),
        Some(&args.mode),
        object([
            (
                "release",
                args.release
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            ),
            (
                "expectedHarnessSha",
                if args.expected_harness_sha.is_empty() {
                    Value::Null
                } else {
                    Value::String(args.expected_harness_sha.clone())
                },
            ),
            (
                "expectedSourceSha",
                expected_source
                    .filter(|value| !value.is_empty())
                    .map(|value| Value::String(value.to_string()))
                    .unwrap_or(Value::Null),
            ),
            ("runs", Value::from(runs.len())),
            ("errors", Value::from(errors.len())),
        ]),
    )?;
    Ok(result)
}

