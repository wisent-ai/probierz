use crate::stado::*;
use serde_json::json;
/// Every remote script packs its retained evidence into the one archive it
/// writes to `$JOB_ROOT/output`; the job's artifacts name it by this extension.
const ARCHIVE_EXTENSION: &str = ".tar.gz";

/// A tar listing's entry as a path inside the harness: tar writes the
/// same entry with or without a leading `./`.
fn inside(entry: &str) -> &str {
    match entry.strip_prefix("./") {
        Some(stripped) => stripped,
        None => entry,
    }
}

pub(crate) fn collection_directory(job_dir: &Path) -> Result<PathBuf, Failure> {
    fs::create_dir_all(job_dir)?;
    // The clock and a nonce of this process and instant name the directory;
    // one that exists already is refused by create_dir, never reused.
    let candidate = job_dir.join(format!(
        "collection-{}-{}",
        now_millis(),
        nonce("collection")
    ));
    fs::create_dir(&candidate)?;
    Ok(candidate)
}

pub(crate) fn fetch_run_evidence(harness: &Path, job_id: &str) -> Result<Retained, Failure> {
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
    );
    let payload: Value = match serde_json::from_str(&output.stdout) {
        Ok(value) => value,
        Err(error) => {
            return Err(remote_failure(
                "stado.download",
                &format!("The queue returned invalid artifact metadata ({error})"),
                &output,
            )
            .discarding(&staging));
        }
    };
    if !output.succeeded || payload.get("ok").and_then(Value::as_bool) != Some(true) {
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
            Failure::discard("stado.download", &staging)?;
            return Ok(Retained {
                results_dir: None,
                manifest: None,
                author_receipt: None,
                author_receipt_file: None,
                artifact_error: upstream,
            });
        }
        return Err(remote_failure(
            "stado.download",
            "Downloading the worker's retained artifacts failed",
            &output,
        )
        .discarding(&staging));
    }
    // An answer without its list is not one that retained nothing.
    let Some(artifacts) = payload
        .pointer("/result/artifacts")
        .and_then(Value::as_array)
        .cloned()
    else {
        return Err(remote_failure(
            "stado.download",
            "Downloading the worker's retained artifacts answered without an artifact list",
            &output,
        )
        .discarding(&staging));
    };
    if artifacts.is_empty() {
        Failure::discard("stado.download", &staging)?;
        return Ok(Retained {
            results_dir: None,
            manifest: None,
            author_receipt: None,
            author_receipt_file: None,
            artifact_error: None,
        });
    }
    let archives: Vec<&str> = artifacts
        .iter()
        .filter_map(|value| value.get("relative_path").and_then(Value::as_str))
        .filter(|path| path.ends_with(ARCHIVE_EXTENSION))
        .collect();
    let artifact_relative = match archives.as_slice() {
        [] => None,
        [archive] => Some(archive.to_string()),
        several => {
            return Err(Failure::config(
                "stado.download",
                format!(
                    "Job {job_id} retained {} archives where its script writes one: {}",
                    several.len(),
                    several.join(", ")
                ),
            )
            .discarding(&staging));
        }
    };
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
        );
        if !listed.succeeded {
            return Err(local_failure(
                "stado.download",
                "Listing the retained evidence archive failed",
                &listed,
            )
            .discarding(&staging));
        }
        entries = listed
            .stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        for entry in &entries {
            let normalized = inside(entry);
            if normalized != "test-results" && !normalized.starts_with("test-results/") {
                return Err(Failure::config(
                    "stado.download",
                    format!("Remote evidence contains an unsafe retained path: {entry}"),
                )
                .discarding(&staging));
            }
            // The path is only checked here; a refusal removes the staging
            // like every other refusal of this collection.
            safe_child(
                harness,
                normalized,
                "Remote evidence contains an unsafe retained path",
            )
            .map_err(|failure| failure.discarding(&staging))?;
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
    );
    if !extracted.succeeded {
        return Err(local_failure(
            "stado.download",
            "Extracting the retained evidence failed",
            &extracted,
        ));
    }
    let author_entry = entries
        .iter()
        .find(|entry| entry.ends_with("/accepted.json"));
    let author_receipt_file = author_entry
        .map(|entry| safe_child(harness, inside(entry), "unsafe author receipt"))
        .transpose()?;
    let author_receipt = match author_receipt_file.as_deref() {
        Some(file) => crate::json_file::read(file)?,
        None => None,
    };
    let selected_run = author_receipt
        .as_ref()
        .and_then(|receipt| receipt.get("runId"))
        .and_then(Value::as_str);
    let mut run_manifest = None;
    for entry in entries
        .iter()
        .filter(|entry| entry.ends_with("/run-manifest.json"))
    {
        let file = safe_child(harness, inside(entry), "unsafe run manifest")?;
        if let Some(candidate) = crate::json_file::read(&file)? {
            // With an author receipt, only the manifest of the run it
            // accepted; without one, the first manifest the archive holds.
            let selected = match selected_run {
                Some(run_id) => candidate.get("runId").and_then(Value::as_str) == Some(run_id),
                None => true,
            };
            if selected {
                run_manifest = Some(candidate);
                break;
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
