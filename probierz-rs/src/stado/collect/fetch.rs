use serde_json::json;
use crate::stado::*;
pub(crate) fn collection_directory(job_dir: &Path) -> Result<PathBuf, Failure> {
    fs::create_dir_all(job_dir)?;
    for sequence in 0..1000_u16 {
        let candidate = job_dir.join(format!("collection-{}-{sequence:03}", now_millis()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(Failure::config(
        "stado.download",
        "could not allocate a unique evidence collection directory",
    ))
}

pub(crate) fn fetch_run_evidence(
    harness: &Path,
    job_id: &str,
    selected: &discovery::Host,
) -> Result<Retained, Failure> {
    let job_dir = harness.join("test-results").join(".remote").join(job_id);
    fs::create_dir_all(&job_dir)?;
    let staging = work_path(&format!(
        "artifacts-{job_id}-{}-{}",
        now_millis(),
        std::process::id()
    ))?;
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    let output = sh(
        STADO_BIN,
        &[
            "machine".into(),
            "artifacts".into(),
            job_id.into(),
            "--output-dir".into(),
            staging.display().to_string(),
        ],
        None,
        Some(selected),
        None,
    );
    let payload: Value = match serde_json::from_str(&output.stdout) {
        Ok(value) => value,
        Err(_) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(remote_failure(
                "stado.download",
                "The queue returned invalid artifact metadata",
                &output,
            ));
        }
    };
    if output.status != Some(0) || payload.get("ok").and_then(Value::as_bool) != Some(true) {
        let upstream = payload.get("error").cloned();
        if upstream
            .as_ref()
            .and_then(|value| value.get("code"))
            .and_then(Value::as_str)
            == Some("NO_ARTIFACTS")
            && upstream
                .as_ref()
                .and_then(|value| value.get("retryable"))
                .and_then(Value::as_bool)
                == Some(false)
        {
            eprintln!(
                "probierz-remote-artifacts {}",
                json!({ "jobId": job_id, "error": upstream })
            );
            let _ = fs::remove_dir_all(&staging);
            return Ok(Retained {
                results_dir: None,
                manifest: None,
                author_receipt: None,
                author_receipt_file: None,
                artifact_error: upstream,
            });
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(remote_failure(
            "stado.download",
            "Downloading the worker's retained artifacts failed",
            &output,
        ));
    }
    let artifacts = payload
        .pointer("/result/artifacts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if artifacts.is_empty() {
        let _ = fs::remove_dir_all(&staging);
        return Ok(Retained {
            results_dir: None,
            manifest: None,
            author_receipt: None,
            author_receipt_file: None,
            artifact_error: None,
        });
    }
    let artifact_relative = artifacts
        .iter()
        .filter_map(|value| value.get("relative_path").and_then(Value::as_str))
        .find(|path| {
            (path.starts_with("probierz-run-")
                || path.starts_with("probierz-author-")
                || path.starts_with("probierz-seo-"))
                && path.ends_with(".tar.gz")
        })
        .map(str::to_string);
    let mut entries = Vec::new();
    if let Some(relative) = &artifact_relative {
        let tarball = safe_child(
            &staging,
            relative,
            "Remote evidence named an artifact outside its collection directory",
        )?;
        let listed = sh(
            "tar",
            &["-tzf".into(), tarball.display().to_string()],
            Some(harness),
            None,
            None,
        );
        if listed.status != Some(0) {
            let _ = fs::remove_dir_all(&staging);
            return Err(local_failure(
                "stado.download",
                "Listing the retained evidence archive failed",
                &listed,
            ));
        }
        entries = listed
            .stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        for entry in &entries {
            let normalized = entry.strip_prefix("./").unwrap_or(entry);
            if normalized != "test-results" && !normalized.starts_with("test-results/") {
                let _ = fs::remove_dir_all(&staging);
                return Err(Failure::config(
                    "stado.download",
                    format!("Remote evidence contains an unsafe retained path: {entry}"),
                ));
            }
            let _ = safe_child(
                harness,
                normalized,
                "Remote evidence contains an unsafe retained path",
            )?;
        }
    }
    let destination = collection_directory(&job_dir)?;
    fs::remove_dir(&destination)?;
    fs::rename(&staging, &destination)?;
    let Some(relative) = artifact_relative else {
        return Ok(Retained {
            results_dir: Some(destination),
            manifest: None,
            author_receipt: None,
            author_receipt_file: None,
            artifact_error: None,
        });
    };
    let tarball = safe_child(
        &destination,
        &relative,
        "Remote evidence named an artifact outside its collection directory",
    )?;
    let extracted = sh(
        "tar",
        &[
            "-xzf".into(),
            tarball.display().to_string(),
            "-C".into(),
            harness.display().to_string(),
        ],
        Some(harness),
        None,
        None,
    );
    if extracted.status != Some(0) {
        return Err(local_failure(
            "stado.download",
            "Extracting the retained evidence failed",
            &extracted,
        ));
    }
    let author_entry = entries
        .iter()
        .find(|entry| entry.ends_with("/accepted.json"));
    let author_receipt_file = author_entry.and_then(|entry| {
        safe_child(
            harness,
            entry.strip_prefix("./").unwrap_or(entry),
            "unsafe author receipt",
        )
        .ok()
    });
    let author_receipt = author_receipt_file.as_deref().and_then(read_json);
    let selected_run = author_receipt
        .as_ref()
        .and_then(|receipt| receipt.get("runId"))
        .and_then(Value::as_str);
    let mut run_manifest = None;
    for entry in entries
        .iter()
        .filter(|entry| entry.ends_with("/run-manifest.json"))
    {
        if let Ok(file) = safe_child(
            harness,
            entry.strip_prefix("./").unwrap_or(entry),
            "unsafe run manifest",
        ) {
            if let Some(candidate) = read_json(&file) {
                if selected_run
                    .map(|run_id| candidate.get("runId").and_then(Value::as_str) == Some(run_id))
                    .unwrap_or(true)
                {
                    run_manifest = Some(candidate);
                    break;
                }
            }
        }
    }
    Ok(Retained {
        results_dir: Some(destination),
        manifest: run_manifest,
        author_receipt,
        author_receipt_file,
        artifact_error: None,
    })
}

