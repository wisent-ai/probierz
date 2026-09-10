use crate::gate::*;
pub fn evaluate(harness: &Path, args: &GateArgs) -> Answer {
    let result = evaluate_value(harness, args)?;
    let passed = truthy(property(&result, "verdict").and_then(|value| property(value, "passed")));
    print_json(&result)?;
    if !passed {
        std::process::exit(1);
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
            ("schemaVersion", Value::from(2)),
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
        let passed =
            truthy(property(&evaluation, "verdict").and_then(|value| property(value, "passed")));
        let error_count = property(&evaluation, "verdict")
            .and_then(|value| property(value, "errors"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
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
    let passed = truthy(property(&result, "verdict").and_then(|value| property(value, "passed")));
    print_json(&result)?;
    if !passed {
        std::process::exit(1);
    }
    Ok(())
}

pub fn activate(harness: &Path, args: &GateArgs) -> Answer {
    let evaluation = evaluate_value(harness, args)?;
    let passed =
        truthy(property(&evaluation, "verdict").and_then(|value| property(value, "passed")));
    if !passed {
        let errors = value_strings(
            property(&evaluation, "verdict").and_then(|value| property(value, "errors")),
        );
        return Err(Failure::invalid(
            "gate.activate",
            format!("gate activation refused: {}", errors.join("; ")),
        ));
    }
    let app = manifest::load(harness, &args.app_id)?;
    let file = config_file(&app);
    let mut current = if file.exists() {
        serde_json::from_str::<Value>(&fs::read_to_string(&file)?)?
    } else {
        default_config(&args.app_id)
    };
    let activated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let receipt_fingerprint = property(&evaluation, "evidence")
        .and_then(|value| property(value, "receipt"))
        .and_then(|value| string_property(value, "fingerprint"))
        .filter(|value| !value.is_empty())
        .map(Value::String)
        .unwrap_or(Value::Null);
    let activation = object([
        ("enforcement", Value::String("required".to_string())),
        ("activatedAt", Value::String(activated_at)),
        (
            "activationEvidence",
            object([
                (
                    "expectedHarnessSha",
                    property(&evaluation, "expectedHarnessSha")
                        .cloned()
                        .unwrap_or(Value::Null),
                ),
                (
                    "expectedSourceSha",
                    property(&evaluation, "expectedSourceSha")
                        .cloned()
                        .unwrap_or(Value::Null),
                ),
                (
                    "release",
                    property(&evaluation, "release")
                        .cloned()
                        .unwrap_or(Value::Null),
                ),
                (
                    "runIds",
                    property(&evaluation, "evidence")
                        .and_then(|value| property(value, "runIds"))
                        .cloned()
                        .unwrap_or_else(|| Value::Array(Vec::new())),
                ),
                (
                    "builds",
                    property(&evaluation, "evidence")
                        .and_then(|value| property(value, "builds"))
                        .cloned()
                        .unwrap_or_else(|| object([])),
                ),
                (
                    "harnessSha256",
                    property(&evaluation, "evidence")
                        .and_then(|value| property(value, "harnessSha256"))
                        .cloned()
                        .unwrap_or(Value::Null),
                ),
                (
                    "sourceSha256",
                    property(&evaluation, "evidence")
                        .and_then(|value| property(value, "sourceSha256"))
                        .cloned()
                        .unwrap_or(Value::Null),
                ),
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
    current_map.insert("schemaVersion".to_string(), Value::from(2));
    let modes = current_map
        .entry("modes")
        .or_insert_with(|| object([]))
        .as_object_mut()
        .ok_or_else(|| Failure::config("gate.activate", "gate modes is not an object"))?;
    modes.insert(args.mode.clone(), activation);
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
        .mode(0o600)
        .open(&temporary)?;
    output.write_all(serde_json::to_string_pretty(&current)?.as_bytes())?;
    output.write_all(b"\n")?;
    drop(output);
    fs::rename(&temporary, &file)?;
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
            (
                "expectedSourceSha",
                args.expected_source_sha
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            ),
            (
                "release",
                args.release
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            ),
        ]),
    )?;
    print_json(&object([
        ("file", Value::String(file.to_string_lossy().into_owned())),
        ("config", current),
        ("evaluation", evaluation),
    ]))
}

