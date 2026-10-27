use crate::gate::*;

/// The version of the verdict document `gate evaluate` and `gate enforce`
/// answer: <https://probierz.wisent.com/docs/cli/gate/evaluate>.
const VERDICT_SCHEMA_VERSION: u64 = 2;
/// The version of the gate configuration `gates.json` that `gate activate`
/// writes: <https://probierz.wisent.com/docs/cli/gate/activate>.
const GATE_CONFIG_SCHEMA_VERSION: u64 = 2;

/// Whether an evaluation's verdict passed. Only JSON `true` passes: a verdict
/// whose `passed` is the text "false", a number or absent blocks the gate.
fn verdict_passed(evaluation: &Value) -> bool {
    is_true(property(evaluation, "verdict").and_then(|value| property(value, "passed")))
}

pub fn evaluate(harness: &Path, args: &GateArgs) -> Answer {
    let result = evaluate_value(harness, args)?;
    let passed = verdict_passed(&result);
    print_json(&result)?;
    if !passed {
        return Err(Failure::new(
            "gate.evaluate",
            crate::failure::Code::Refused,
            result.to_string(),
        ));
    }
    Ok(())
}

pub fn enforce(harness: &Path, args: &GateArgs) -> Answer {
    let status = gate_status_value(harness, &args.app_id)?;
    let enforcement = property(&status, "modes")
        .and_then(|modes| property(modes, &args.mode))
        .and_then(|mode| string_property(mode, "enforcement"));
    let result = if enforcement.as_deref() != Some("required") {
        let result = object([
            ("schemaVersion", Value::from(VERDICT_SCHEMA_VERSION)),
            ("appId", Value::String(args.app_id.clone())),
            ("mode", Value::String(args.mode.clone())),
            (
                "verdict",
                object([
                    ("passed", Value::Bool(false)),
                    (
                        "errors",
                        Value::Array(vec![Value::String(
                            "gate is pending green activation".to_string(),
                        )]),
                    ),
                ]),
            ),
            ("status", status),
        ]);
        audit_access(
            harness,
            "gate.enforce",
            "denied",
            Some(&args.app_id),
            Some(&args.mode),
            object([("reason", Value::String("pending-green".to_string()))]),
        )?;
        result
    } else {
        let mut evaluation = evaluate_value(harness, args)?;
        let passed = verdict_passed(&evaluation);
        // Every evaluation writes its verdict's error list; one without it
        // is refused rather than audited as an enforcement with no errors.
        let error_count = property(&evaluation, "verdict")
            .and_then(|value| property(value, "errors"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .ok_or_else(|| {
                Failure::config(
                    "gate.enforce",
                    "the evaluation's verdict states no error list",
                )
            })?;
        audit_access(
            harness,
            "gate.enforce",
            if passed { "allowed" } else { "denied" },
            Some(&args.app_id),
            Some(&args.mode),
            object([("errors", Value::from(error_count))]),
        )?;
        evaluation
            .as_object_mut()
            .ok_or_else(|| Failure::config("gate.enforce", "evaluation is not an object"))?
            .insert("status".to_string(), status);
        evaluation
    };
    let passed = verdict_passed(&result);
    print_json(&result)?;
    if !passed {
        return Err(Failure::new(
            "gate.enforce",
            crate::failure::Code::Refused,
            result.to_string(),
        ));
    }
    Ok(())
}

/// A field of a green evaluation that activation records, copied as the
/// evaluation states it (null included). An evaluation that does not state
/// it is refused: the activation would otherwise record evidence nobody read.
fn stated(evaluation: &Value, path: &[&str]) -> Result<Value, Failure> {
    let mut at = Some(evaluation);
    for key in path {
        at = at.and_then(|value| property(value, key));
    }
    at.cloned().ok_or_else(|| {
        Failure::config(
            "gate.activate",
            format!("the green evaluation states no {}", path.join(".")),
        )
    })
}

/// Replace `file` with `config` in one rename, through a sibling only the
/// owner can read and write.
fn write_config(file: &Path, config: &Value) -> Result<(), Failure> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = PathBuf::from(format!(
        "{}.tmp-{}-{}",
        file.to_string_lossy(),
        std::process::id(),
        Utc::now().timestamp_millis()
    ));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(u32::from(libc::S_IRUSR | libc::S_IWUSR))
        .open(&temporary)?;
    output.write_all(serde_json::to_string_pretty(config)?.as_bytes())?;
    output.write_all(b"\n")?;
    drop(output);
    fs::rename(&temporary, file)?;
    Ok(())
}

pub fn activate(harness: &Path, args: &GateArgs) -> Answer {
    let evaluation = evaluate_value(harness, args)?;
    if !verdict_passed(&evaluation) {
        let errors = value_strings(
            property(&evaluation, "verdict").and_then(|value| property(value, "errors")),
            "gate evaluation verdict.errors",
        ).map_err(|detail| Failure::config("gate.activate", detail))?;
        return Err(Failure::invalid(
            "gate.activate",
            format!("gate activation refused: {}", errors.join("; ")),
        ));
    }
    let app = manifest::load(harness, &args.app_id)?;
    let file = config_file(&app)?;
    let mut current = match crate::json_file::read(&file)? {
        Some(current) => current,
        None => default_config(&args.app_id),
    };
    let activated_at = crate::failure::now_iso();
    // A pull-request evaluation carries no receipt; an empty fingerprint is
    // no fingerprint.
    let receipt_fingerprint = Value::from(
        property(&evaluation, "evidence")
            .and_then(|value| property(value, "receipt"))
            .and_then(|value| string_property(value, "fingerprint"))
            .filter(|value| !value.is_empty()),
    );
    let activation = object([
        ("enforcement", Value::String("required".to_string())),
        ("activatedAt", Value::String(activated_at)),
        (
            "activationEvidence",
            object([
                ("expectedHarnessSha", stated(&evaluation, &["expectedHarnessSha"])?),
                ("expectedSourceSha", stated(&evaluation, &["expectedSourceSha"])?),
                ("release", stated(&evaluation, &["release"])?),
                ("runIds", stated(&evaluation, &["evidence", "runIds"])?),
                ("builds", stated(&evaluation, &["evidence", "builds"])?),
                ("harnessSha256", stated(&evaluation, &["evidence", "harnessSha256"])?),
                ("sourceSha256", stated(&evaluation, &["evidence", "sourceSha256"])?),
                ("receiptFingerprint", receipt_fingerprint),
            ]),
        ),
    ]);
    let current_map = current.as_object_mut().ok_or_else(|| {
        Failure::config(
            "gate.activate",
            format!("{} does not contain a gate object", file.display()),
        )
    })?;
    current_map.insert(
        "schemaVersion".to_string(),
        Value::from(GATE_CONFIG_SCHEMA_VERSION),
    );
    let modes = current_map
        .entry("modes")
        .or_insert_with(|| object([]))
        .as_object_mut()
        .ok_or_else(|| Failure::config("gate.activate", "gate modes is not an object"))?;
    modes.insert(args.mode.clone(), activation);
    write_config(&file, &current)?;
    audit_access(
        harness,
        "gate.activate",
        "allowed",
        Some(&args.app_id),
        Some(&args.mode),
        object([
            (
                "expectedHarnessSha",
                Value::String(args.expected_harness_sha.clone()),
            ),
            ("expectedSourceSha", Value::from(args.expected_source_sha.clone())),
            ("release", Value::from(args.release.clone())),
        ]),
    )?;
    print_json(&object([
        ("file", Value::String(file.to_string_lossy().into_owned())),
        ("config", current),
        ("evaluation", evaluation),
    ]))
}

/// The inverse of [`activate`]: one mode's activation leaves the gate
/// configuration, so the policy is evaluated but no longer enforced. The
/// write is atomic like activation's and audited with its reason; a mode
/// that is not active is reported unchanged and nothing is written.
pub fn deactivate(harness: &Path, args: &DeactivateArgs) -> Answer {
    if args.reason.trim().is_empty() {
        return Err(Failure::invalid(
            "gate.deactivate",
            "--reason must say why enforcement is withdrawn",
        ));
    }
    let app = manifest::load(harness, &args.app_id)?;
    let file = config_file(&app)?;
    let Some(mut current) = crate::json_file::read(&file)? else {
        return print_json(&object([
            ("file", Value::String(file.to_string_lossy().into_owned())),
            ("deactivated", Value::Bool(false)),
        ]));
    };
    let removed = current
        .get_mut("modes")
        .and_then(Value::as_object_mut)
        .and_then(|modes| modes.remove(&args.mode))
        .is_some();
    if removed {
        write_config(&file, &current)?;
        audit_access(
            harness,
            "gate.deactivate",
            "allowed",
            Some(&args.app_id),
            Some(&args.mode),
            object([("reason", Value::String(args.reason.clone()))]),
        )?;
    }
    print_json(&object([
        ("file", Value::String(file.to_string_lossy().into_owned())),
        ("deactivated", Value::Bool(removed)),
        ("config", current),
    ]))
}
