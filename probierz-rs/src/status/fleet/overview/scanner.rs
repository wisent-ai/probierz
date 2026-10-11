use crate::status::*;

pub(super) fn violations_for(root: &str) -> Value {
    let output = stado_wait::output(Command::new("tama").args([
        "violations",
        "find",
        "--repo",
        root,
        "--json",
    ]));
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            return json!({ "error": format!("tama violations find cannot be run for {root}: {error}") })
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let refused = |cause: String| {
        json!({
            "error": format!(
                "tama violations find for {root} {}: {cause}; stdout: {stdout}; stderr: {stderr}",
                crate::failure::ended(&output.status)
            )
        })
    };
    if !output.status.success() && !stdout.trim().starts_with('{') {
        return refused("the scanner did not return a JSON report".to_string());
    }
    let report = match serde_json::from_str::<Value>(&stdout) {
        Ok(report) => report,
        Err(error) => return refused(format!("invalid JSON: {error}")),
    };
    match report.get("problems").and_then(Value::as_array) {
        None => return refused("the report has no problems array".to_string()),
        Some(problems) if !problems.is_empty() => {
            return refused(format!("the scan reported problems {}", report["problems"]))
        }
        Some(_) => {}
    }
    let Some(repositories) = report.get("repos").and_then(Value::as_array) else {
        return refused("the report has no repos array".to_string());
    };
    let [repo] = repositories.as_slice() else {
        return refused(format!(
            "the scan must identify exactly the requested repository; received {} repositories",
            repositories.len()
        ));
    };
    let Some(scanned) = repo.get("repo").and_then(Value::as_str) else {
        return refused("the repository report has no repo path".to_string());
    };
    let requested = match Path::new(root).canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return refused(format!(
                "cannot resolve requested repository {root}: {error}"
            ))
        }
    };
    let observed = match Path::new(scanned).canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return refused(format!(
                "cannot resolve scanned repository {scanned}: {error}"
            ))
        }
    };
    if requested != observed {
        return refused(format!(
            "scanned repository {} differs from requested repository {}",
            observed.display(),
            requested.display()
        ));
    }
    let count = |field: &str| {
        repo.get(field)
            .and_then(Value::as_array)
            .map(Vec::len)
            .ok_or_else(|| format!("the repository report has no {field} array"))
    };
    match (count("violations"), count("skippedFiles"), count("errors")) {
        (Ok(violations), Ok(skipped), Ok(errors)) => {
            json!({ "violations": violations, "skipped": skipped, "errors": errors })
        }
        (Err(cause), _, _) | (_, Err(cause), _) | (_, _, Err(cause)) => refused(cause),
    }
}
