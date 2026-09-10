use serde_json::json;
use crate::stado::*;
pub(crate) fn write_author_receipt(harness: &Path, args: AuthorReceiptArgs) -> Answer {
    let receipt_id = safe_author_name(&args.receipt_id, "authoring receipt")?;
    let application = manifest::load(harness, &args.app)?;
    let test_directory = application
        .document
        .get("surfaces")
        .and_then(|value| value.get(&args.target))
        .and_then(|value| value.get("testDirectory"))
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("tests");
    let result = read_json(&args.result).ok_or_else(|| {
        Failure::config(
            "stado.author-receipt",
            "Remote authoring did not return a readable result.",
        )
    })?;
    if result.get("ok").and_then(Value::as_bool) != Some(true)
        || result.get("journey").and_then(Value::as_str) != Some(args.journey.as_str())
        || result.get("target").and_then(Value::as_str) != Some(args.target.as_str())
    {
        return Err(Failure::config(
            "stado.author-receipt",
            "Remote authoring did not return an accepted spec.",
        ));
    }
    let run_id = result.get("runId").and_then(Value::as_str).ok_or_else(|| {
        Failure::config(
            "stado.author-receipt",
            "Remote authoring did not return its verification run.",
        )
    })?;
    let registration_dir = registration_directory(&args.target).ok_or_else(|| {
        Failure::config(
            "stado.author-receipt",
            format!(
                "Remote authoring does not support target \"{}\".",
                args.target
            ),
        )
    })?;
    let registration_relative = format!(
        "{registration_dir}/{}-{}{}",
        args.app,
        args.journey,
        registration_extension(&args.target),
    );
    let returned_spec = result
        .get("spec")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| {
            Failure::config(
                "stado.author-receipt",
                "Remote authoring did not return its accepted spec path.",
            )
        })?;
    if returned_spec != harness.join(&registration_relative) || !returned_spec.is_file() {
        return Err(Failure::config(
            "stado.author-receipt",
            "Remote authoring returned an accepted spec outside its registration path.",
        ));
    }
    let run_manifest = harness
        .join("test-results")
        .join(run_id)
        .join("run-manifest.json");
    let retained_manifest = read_json(&run_manifest).ok_or_else(|| {
        Failure::config(
            "stado.author-receipt",
            "Remote authoring completed without its verification manifest.",
        )
    })?;
    if retained_manifest.get("runId").and_then(Value::as_str) != Some(run_id)
        || retained_manifest.get("appId").and_then(Value::as_str) != Some(args.app.as_str())
        || retained_manifest.get("target").and_then(Value::as_str) != Some(args.target.as_str())
        || retained_manifest.get("status").and_then(Value::as_str) != Some("passed")
    {
        return Err(Failure::config(
            "stado.author-receipt",
            "Remote authoring verification did not retain a passing source-bound manifest.",
        ));
    }
    let identity_file = std::env::var_os("PROBIERZ_SOURCE_IDENTITY")
        .map(PathBuf::from)
        .ok_or_else(|| {
            Failure::config(
                "stado.author-receipt",
                "Remote authoring has no submitting source identity.",
            )
        })?;
    let identity = read_json(&identity_file).ok_or_else(|| {
        Failure::config(
            "stado.author-receipt",
            "Remote authoring has no readable submitting source identity.",
        )
    })?;
    if identity.get("appId").and_then(Value::as_str) != Some(args.app.as_str()) {
        return Err(Failure::config(
            "stado.author-receipt",
            "Remote authoring source identity does not name the submitted app.",
        ));
    }
    let source_sha = identity
        .pointer("/app/sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Failure::config(
                "stado.author-receipt",
                "Remote authoring source identity has no app digest.",
            )
        })?;
    let harness_sha = identity
        .pointer("/harness/sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Failure::config(
                "stado.author-receipt",
                "Remote authoring source identity has no harness digest.",
            )
        })?;
    if retained_manifest
        .pointer("/source/sha256")
        .and_then(Value::as_str)
        != Some(source_sha)
        || retained_manifest
            .pointer("/harness/sha256")
            .and_then(Value::as_str)
            != Some(harness_sha)
        || retained_manifest
            .get("sourceIdentityOrigin")
            .and_then(Value::as_str)
            != Some("submitter")
    {
        return Err(Failure::config(
            "stado.author-receipt",
            "Remote authoring verification does not match the submitting source identity.",
        ));
    }
    let accepted = fs::read(&returned_spec)?;
    let digest = hex::encode(Sha256::digest(&accepted));
    let receipt_root = harness
        .join("test-results")
        .join(".authoring")
        .join(&args.app)
        .join(receipt_id);
    fs::create_dir_all(&receipt_root)?;
    let artifact_file =
        receipt_root.join(format!("accepted-spec.{}", product_extension(&args.target)));
    fs::write(&artifact_file, &accepted)?;
    let artifact_relative = artifact_file
        .strip_prefix(harness)
        .map_err(|_| {
            Failure::config(
                "stado.author-receipt",
                "Accepted spec is outside retained Probierz artifacts.",
            )
        })?
        .to_string_lossy()
        .to_string();
    let product_relative = format!(
        "{}/{}/{}.probierz.spec.{}",
        test_directory,
        args.area,
        args.journey,
        product_extension(&args.target),
    );
    let receipt = json!({
        "schemaVersion": 1,
        "appId": args.app,
        "journey": args.journey,
        "area": args.area,
        "target": args.target,
        "runId": run_id,
        "sourceSha256": source_sha,
        "harnessSha256": harness_sha,
        "spec": {
            "relativePath": product_relative,
            "artifact": artifact_relative,
            "bytes": accepted.len(),
            "sha256": digest,
        },
        "registration": { "relativePath": registration_relative },
        "mappingPaths": [],
    });
    let receipt_file = receipt_root.join("accepted.json");
    write_json(&receipt_file, &receipt, true, true)?;
    fs::set_permissions(&receipt_file, fs::Permissions::from_mode(0o600))?;
    print_json(&json!({ "ok": true, "receipt": receipt_file }))
}

