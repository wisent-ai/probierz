use serde_json::json;
use crate::stado::*;
pub(crate) fn require_local_kind(path: &Path, directory: bool, name: &str) -> Answer {
    if !path.is_absolute() || !path.exists() {
        return Err(Failure::config(
            "byk.remote",
            format!("{name} must be an existing absolute path"),
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(Failure::config(
            "byk.remote",
            format!("{name} has the wrong file type"),
        ));
    }
    Ok(())
}

pub(crate) fn byk_state_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Caches")
        .join("probierz")
        .join("remote-hosts")
        .join("byk-auth.json")
}

pub(crate) fn assert_byk_host_available(home: &Path) -> Answer {
    let Some(state) = read_json(&byk_state_path(home)) else {
        return Ok(());
    };
    let Some(until) = state
        .get("quarantinedUntil")
        .and_then(Value::as_str)
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
    else {
        return Ok(());
    };
    if until.with_timezone(&Utc) > Utc::now() {
        return Err(Failure::unavailable(
            "byk.remote",
            format!("dedicated host is quarantined until {}", until.to_rfc3339()),
        ));
    }
    Ok(())
}

pub(crate) fn quarantine_byk_host(home: &Path, selector: &str, reason: &str) -> Answer {
    let file = byk_state_path(home);
    let previous = read_json(&file);
    let now = Utc::now();
    let value = json!({
        "schemaVersion": 1,
        "host": selector,
        "failures": previous.as_ref().and_then(|value| value.get("failures")).and_then(Value::as_u64).unwrap_or(0) + 1,
        "reason": reason,
        "quarantinedAt": now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "quarantinedUntil": (now + chrono::Duration::from_std(BYK_QUARANTINE).unwrap_or_default())
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    });
    let parent = file.parent().expect("state parent");
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    let temporary = file.with_extension(format!("{}.{}.tmp", std::process::id(), nonce("byk")));
    write_json(&temporary, &value, true, true)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    fs::rename(temporary, file)?;
    Ok(())
}

pub(crate) fn clear_byk_quarantine(home: &Path) -> Answer {
    let file = byk_state_path(home);
    if file.exists() {
        fs::remove_file(file)?;
    }
    Ok(())
}

pub(crate) fn retry_byk<F>(label: &str, mut operation: F) -> Answer
where
    F: FnMut() -> ProcessOutput,
{
    let mut last = None;
    for attempt in 0..BYK_RETRIES {
        let output = operation();
        if output.status == Some(0) {
            return Ok(());
        }
        last = Some(output);
        if attempt + 1 < BYK_RETRIES {
            thread::sleep(Duration::from_secs(1_u64 << attempt));
        }
    }
    let detail = last
        .as_ref()
        .map(process_text)
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "exit unknown".into());
    Err(Failure::unavailable(
        "byk.remote",
        format!("{label} failed after {BYK_RETRIES} attempts ({detail})"),
    ))
}

