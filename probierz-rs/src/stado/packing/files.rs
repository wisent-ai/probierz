use crate::stado::*;
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Milliseconds since the Unix epoch, as chrono reads the clock: a clock
/// before the epoch is a negative count, never a silent zero.
pub(crate) fn now_millis() -> i64 {
    Utc::now().timestamp_millis()
}

pub(crate) fn nonce(prefix: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(prefix.as_bytes());
    digest.update(now_millis().to_string().as_bytes());
    digest.update(std::process::id().to_string().as_bytes());
    digest.update(format!("{:?}", Instant::now()).as_bytes());
    hex::encode(digest.finalize())
}

pub(crate) fn work_path(name: &str) -> Result<PathBuf, Failure> {
    let directory = crate::run::home_dir()
        .join(".stado")
        .join("work")
        .join("probierz");
    fs::create_dir_all(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(u32::from(libc::S_IRWXU)))?;
    Ok(directory.join(name))
}

pub(crate) fn write_json(
    path: &Path,
    value: &Value,
    pretty: bool,
    newline: bool,
) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = if pretty {
        serde_json::to_vec_pretty(value)?
    } else {
        serde_json::to_vec(value)?
    };
    if newline {
        bytes.push(b'\n');
    }
    fs::write(path, bytes)?;
    Ok(())
}

pub(crate) fn hash_file(path: &Path) -> Result<String, Failure> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    std::io::copy(&mut file, &mut digest)?;
    Ok(hex::encode(digest.finalize()))
}

pub(crate) fn pack_source_tree(root: &Path, file: &Path, label: &str) -> Result<(), Failure> {
    let list = work_path(&format!(
        "source-list-{}-{}",
        std::process::id(),
        nonce(label)
    ))?;
    fs::write(&list, source_file_list(root)?)
        .map_err(|error| Failure::from(error).discarding(&list))?;
    fs::set_permissions(&list, fs::Permissions::from_mode(u32::from(libc::S_IRUSR | libc::S_IWUSR)))
        .map_err(|error| Failure::from(error).discarding(&list))?;
    let args = vec![
        "-czf".to_string(),
        file.display().to_string(),
        "--null".to_string(),
        "-T".to_string(),
        list.display().to_string(),
    ];
    let output = sh("tar", &args, Some(root));
    if !output.succeeded {
        return Err(
            local_failure("stado.pack", &format!("Packing {label} failed"), &output)
                .discarding(&list),
        );
    }
    Failure::discard("stado.pack", &list)
}

pub(crate) fn pack_repo(harness: &Path, app_ids: &[&str]) -> Result<Packed, Failure> {
    for app_id in app_ids {
        if !harness.join("apps").join(app_id).exists() {
            return Err(Failure::config(
                "stado.pack",
                format!("No app manifest for \"{app_id}\". Register it under apps/ before submitting a remote run."),
            ));
        }
    }
    let hash = nonce("probierz");
    let file = work_path(&format!("probierz-{hash}.tar.gz"))?;
    pack_source_tree(harness, &file, "the probierz checkout")?;
    Ok(Packed { file, hash })
}

pub(crate) fn pack_app_source(app_id: &str, repository: &Path) -> Result<Packed, Failure> {
    let hash = nonce(app_id);
    let file = work_path(&format!("{app_id}-{hash}.tar.gz"))?;
    pack_source_tree(repository, &file, &format!("the {app_id} source tree"))?;
    Ok(Packed { file, hash })
}

pub(crate) fn pack_app_bundle(app_id: &str, bundle: &Path) -> Result<(Packed, String), Failure> {
    let name = bundle
        .file_name()
        .and_then(|part| part.to_str())
        .ok_or_else(|| Failure::config("stado.pack", "application bundle has no file name"))?
        .to_string();
    let parent = bundle.parent().ok_or_else(|| {
        Failure::config("stado.pack", "application bundle has no parent directory")
    })?;
    let hash = nonce(&format!("{app_id}-app"));
    let file = work_path(&format!("{app_id}-app-{hash}.tar.gz"))?;
    let args = vec![
        "-czf".into(),
        file.display().to_string(),
        "-C".into(),
        parent.display().to_string(),
        name.clone(),
    ];
    let output = sh("tar", &args, None);
    if !output.succeeded {
        return Err(local_failure(
            "stado.pack",
            &format!("Packing the {app_id} application bundle failed"),
            &output,
        ));
    }
    Ok((Packed { file, hash }, name))
}

pub(crate) fn pack_source_identity(
    harness: &Path,
    app_id: &str,
    app_repo: Option<&Path>,
) -> Result<Identity, Failure> {
    let document = crate::authoring::app_source_identity(harness, app_id, app_repo)?;
    let compact = serde_json::to_vec(&document)?;
    let hash = hex::encode(Sha256::digest(&compact));
    let file = work_path(&format!("{app_id}-source-{hash}.json"))?;
    write_json(&file, &document, true, true)?;
    Ok(Identity {
        document,
        file,
        hash,
    })
}

/// Put one file into the Stado object store, once. A refused put is the
/// failure, reported with Stado's own output; nothing is retried or slept on.
pub(crate) fn upload(local_file: &Path, name: &str) -> Result<String, Failure> {
    let destination = format!("{}/{name}", state_uri("inputs"));
    let mut output = sh(
        STADO_BIN,
        &[
            "storage".into(),
            "put".into(),
            destination.clone(),
            local_file.display().to_string(),
        ],
        None,
    );
    if output.succeeded {
        return Ok(destination);
    }
    output
        .stderr
        .push_str(&format!(" (source {})", local_file.display()));
    Err(remote_failure(
        "stado.upload",
        &format!("Uploading {name} to the stado object store failed"),
        &output,
    ))
}
