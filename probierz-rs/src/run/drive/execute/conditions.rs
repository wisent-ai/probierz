use crate::run::*;

/// The declared surface a run is about to drive, with the conditions it
/// brings: the app's own condition map, the secrets its declaration names,
/// and the environment aliases it asks for — each folded into `opts.env`
/// without overwriting what the caller already passed.
pub(crate) fn declared_surface(
    harness: &Path,
    app_id: &str,
    name: &str,
    opts: &mut RunOptions,
) -> Result<(Option<manifest::Manifest>, Option<serde_yaml::Value>), Failure> {
    if app_id == "probierz" {
        return Ok((None, None));
    }
    let (declaration, value) = app_surface(harness, app_id, name)?;
    let mut conditions = yaml_map_strings(value.get("conditions"));
    conditions.extend(opts.env.clone());
    opts.env = conditions;
    for secret in declaration
        .document
        .get("secretRefs")
        .and_then(serde_yaml::Value::as_mapping)
        .into_iter()
        .flatten()
        .filter_map(|(key, _)| key.as_str())
    {
        if !opts.env.contains_key(secret) {
            if let Ok(value) = std::env::var(secret) {
                opts.env.insert(secret.into(), value);
            }
        }
    }
    for (target_name, source_name) in value
        .get("env")
        .and_then(serde_yaml::Value::as_mapping)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| Some((key.as_str()?, value.as_str()?)))
    {
        if let Some(value) = opts
            .env
            .get(source_name)
            .cloned()
            .or_else(|| std::env::var(source_name).ok())
        {
            opts.env.insert(source_name.into(), value.clone());
            opts.env.insert(target_name.into(), value);
        }
    }
    Ok((Some(declaration), Some(value)))
}

/// The environment the suite process is started with: the caller's snapshot
/// plus every coordinate the drivers read out of the environment rather than
/// out of arguments, including the spec a surface declared. A Byk run names
/// its spec to the broker instead, so it is not exported here.
pub(crate) fn suite_environment(
    harness: &Path,
    app_id: &str,
    run_id: &str,
    artifacts: &Path,
    report_path: &Path,
    journeys: &[String],
    record: bool,
    spec: Option<&str>,
    byk: bool,
    opts: &RunOptions,
) -> BTreeMap<String, String> {
    let mut env = env_snapshot(&opts.env);
    env.insert("PROBIERZ_APP_ID".into(), app_id.to_string());
    env.insert("PROBIERZ_RUN_ID".into(), run_id.to_string());
    env.insert(
        "PROBIERZ_TOOLKIT_ROOT".into(),
        harness.to_string_lossy().into_owned(),
    );
    env.insert(
        "PROBIERZ_ARTIFACTS".into(),
        artifacts.to_string_lossy().into_owned(),
    );
    env.insert(
        "PROBIERZ_REPORT_PATH".into(),
        report_path.to_string_lossy().into_owned(),
    );
    env.insert("PROBIERZ_JOURNEYS".into(), journeys.join(","));
    env.insert(
        "PROBIERZ_NATIVE_CAPTURE_BIN".into(),
        harness
            .join("node_modules/.cache/probierz/screen-capture-kit")
            .to_string_lossy()
            .into_owned(),
    );
    if record {
        env.insert("PROBIERZ_RECORD".into(), "1".into());
    }
    if let Ok(binary) = std::env::current_exe() {
        env.insert("PROBIERZ_BIN".into(), binary.to_string_lossy().into_owned());
    }
    if let Some(spec) = spec.filter(|_| !byk) {
        env.insert("PROBIERZ_SPEC".into(), spec.to_string());
    }
    env
}
