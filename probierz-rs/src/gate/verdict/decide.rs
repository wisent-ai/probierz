use crate::gate::*;

#[path = "policy.rs"]
mod policy;

/// The verdict document version: <https://probierz.wisent.com/docs/cli/gate/evaluate>.
const VERDICT_SCHEMA_VERSION: u64 = 2;

pub(crate) fn evaluate_value(harness: &Path, args: &GateArgs) -> Result<Value, Failure> {
    if args.app_id.is_empty() || !matches!(args.mode.as_str(), "pull-request" | "release") {
        return Err(Failure::invalid(
            "gate.evaluate",
            "gate needs an app ID and pull-request or release mode",
        ));
    }
    let app = manifest::load(harness, &args.app_id)?;
    let policy = policy::Policy::read(&app, &args.mode)?;
    let (minimum_evidence, required_rank) = policy.minimum_evidence()?;
    let required_targets = policy.texts("requiredTargets")?;
    let required_journeys = policy.texts("requiredJourneys")?;
    let matrix_profile = policy.text("requiredMatrixProfile")?;
    let require_protected = policy.flag("requireProtectedArtifacts")?;
    let require_secret_scan = policy.flag("requireSecretScan")?;
    let run_ids: Vec<String> = match args.runs.as_deref() {
        Some(runs) => runs
            .split(',')
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        None => Vec::new(),
    };
    let mut errors = Vec::new();
    if args.expected_harness_sha.is_empty() {
        errors.push("expected harness source SHA-256 is required".to_string());
    }
    if !args
        .expected_source_sha
        .as_deref()
        .is_some_and(|value| !value.is_empty())
    {
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
    let mut sources = source_hashes.iter();
    let source_sha = match (sources.next(), sources.next()) {
        (Some(hash), None) => Value::String(hash.clone()),
        _ => Value::Null,
    };
    if !runs.is_empty() && source_sha.is_null() {
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
        if !coverage.missing.is_empty() {
            errors.push(format!(
                "{} required matrix cell(s) are missing",
                coverage.missing.len()
            ));
        }
        if !coverage.extra_run_ids.is_empty() {
            errors.push(format!(
                "{} run(s) are outside the required matrix",
                coverage.extra_run_ids.len()
            ));
        }
        coverage.record()
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
    let mut levels = Map::new();
    for run in &runs {
        levels.insert(
            run.run_id.clone(),
            Value::String(evidence_level(run).to_string()),
        );
    }
    let result = object([
        ("schemaVersion", Value::from(VERDICT_SCHEMA_VERSION)),
        ("appId", Value::String(args.app_id.clone())),
        ("mode", Value::String(args.mode.clone())),
        (
            "release",
            match &args.release {
                Some(release) => Value::String(release.clone()),
                None => Value::Null,
            },
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
            match expected_source.filter(|value| !value.is_empty()) {
                Some(source) => Value::String(source.to_string()),
                None => Value::Null,
            },
        ),
        (
            "policy",
            object([
                ("minimumEvidence", Value::String(minimum_evidence)),
                ("requiredTargets", strings(&required_targets)),
                ("requiredJourneys", strings(&required_journeys)),
                (
                    "requiredMatrixProfile",
                    match matrix_profile {
                        Some(profile) => Value::String(profile),
                        None => Value::Null,
                    },
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
                match &args.release {
                    Some(release) => Value::String(release.clone()),
                    None => Value::Null,
                },
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
                match expected_source.filter(|value| !value.is_empty()) {
                    Some(source) => Value::String(source.to_string()),
                    None => Value::Null,
                },
            ),
            ("runs", Value::from(runs.len())),
            ("errors", Value::from(errors.len())),
        ]),
    )?;
    Ok(result)
}
