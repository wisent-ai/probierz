use crate::stado::*;
pub(crate) fn protected_byk_child(root: &Path, candidate: &str, name: &str) -> Result<PathBuf, Failure> {
    let candidate = PathBuf::from(candidate);
    if !candidate.is_absolute() || !candidate.starts_with(root) || candidate == root {
        return Err(Failure::config(
            "byk.worker",
            format!("{name} must stay inside the protected run directory"),
        ));
    }
    Ok(candidate)
}

pub(crate) fn valid_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !value.chars().any(char::is_whitespace)
}

pub(crate) fn base_byk_environment() -> BTreeMap<String, String> {
    const NAMES: &[&str] = &[
        "PATH",
        "HOME",
        "TMPDIR",
        "USER",
        "SHELL",
        "LANG",
        "TERM",
        "COLORTERM",
        "FORCE_COLOR",
        "NO_COLOR",
        "CLICOLOR",
        "CLICOLOR_FORCE",
        "APPIUM_HOME",
        "DEVELOPER_DIR",
        "SDKROOT",
        "TOOLCHAINS",
        "XCODE_DEFAULT_TOOLCHAIN_OVERRIDE",
        "XCODE_DEVELOPER_USR_PATH",
        "XCODE_PRODUCT_BUILD_VERSION",
        "XCODE_TOOLCHAIN_PATH",
        "XCODE_VERSION_ACTUAL",
        "XCODE_VERSION_MAJOR",
        "XCODE_VERSION_MINOR",
        "XCODE_XCCONFIG_FILE",
        "IOS_DEVICE",
        "IOS_VERSION",
        "CI",
    ];
    let mut environment = BTreeMap::new();
    for (name, value) in std::env::vars() {
        if NAMES.contains(&name.as_str()) || name.starts_with("LC_") {
            environment.insert(name, value);
        }
    }
    let system = "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin";
    let path = environment
        .get("PATH")
        .map(|value| format!("{system}:{value}"))
        .unwrap_or_else(|| system.into());
    environment.insert("PATH".into(), path);
    environment
}

pub(crate) fn worker_status(
    command: &str,
    args: &[&str],
    cwd: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<i32, Failure> {
    let mut child = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| Failure::config("byk.worker", format!("could not start {command}")))?;
    ACTIVE_CHILD.store(child.id() as i32, Ordering::SeqCst);
    let status = child.wait()?;
    ACTIVE_CHILD.store(0, Ordering::SeqCst);
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use super::*;

    fn process(status: i32) -> ProcessOutput {
        ProcessOutput {
            command: "stado".into(),
            args: Vec::new(),
            status: Some(status),
            signal: None,
            stdout: String::new(),
            stderr: String::new(),
            error: None,
        }
    }

    #[test]
    fn machine_request_has_the_stado_protocol_shape_and_only_secret_coordinates() {
        let selected = discovery::stado_host("stado:gcp").expect("host");
        let mut inputs = Map::new();
        inputs.insert(
            "repo".into(),
            json!({
                "stado_uri": "stado://probierz/inputs/probierz-fixed.tar.gz",
                "relative_path": "inputs/probierz.tar.gz",
            }),
        );
        let mut request = Map::new();
        request.insert(
            "client_request_id".into(),
            Value::String("probierz-run-fixed".into()),
        );
        request.insert(
            "command".into(),
            Value::String("PROBIERZ_WATCH_BUDGET_MS=1234 bash inputs/run.sh".into()),
        );
        request.insert(
            "output_uri".into(),
            Value::String("stado://probierz/results".into()),
        );
        request.insert("input_objects".into(), Value::Object(inputs));
        request.insert(
            "secret_env".into(),
            json!({
                "STADO_MODEL_ROUTER_TOKEN": { "item": "probierz-model-router", "field": "token" },
            }),
        );
        for (name, value) in selected
            .request
            .expect("request")
            .as_object()
            .expect("object")
        {
            request.insert(name.clone(), value.clone());
        }
        assert_eq!(
            serde_json::to_string(&Value::Object(request)).expect("json"),
            r#"{"client_request_id":"probierz-run-fixed","command":"PROBIERZ_WATCH_BUDGET_MS=1234 bash inputs/run.sh","output_uri":"stado://probierz/results","input_objects":{"repo":{"stado_uri":"stado://probierz/inputs/probierz-fixed.tar.gz","relative_path":"inputs/probierz.tar.gz"}},"secret_env":{"STADO_MODEL_ROUTER_TOKEN":{"item":"probierz-model-router","field":"token"}},"provider":"gcp","pin_to_provider":true}"#,
        );
    }

    #[test]
    fn upload_retries_six_times_with_five_seconds_more_each_time() {
        let mut calls = 0;
        let mut delays = Vec::new();
        let error = upload_with(
            Path::new("/tmp/input"),
            "input.tar.gz",
            |_, _| {
                calls += 1;
                process(STADO_RETRY_EXIT)
            },
            |delay| delays.push(delay),
        )
        .expect_err("upload must fail");
        assert_eq!(calls, 6);
        assert_eq!(
            delays,
            vec![
                Duration::from_secs(5),
                Duration::from_secs(10),
                Duration::from_secs(15),
                Duration::from_secs(20),
                Duration::from_secs(25),
            ]
        );
        assert_eq!(error.point, "stado.upload");
    }

    #[test]
    fn environment_names_are_shell_identifiers_and_values_may_contain_equals() {
        assert_eq!(
            parse_environment(&["GOOD_name=a=b".into()]).expect("valid"),
            vec![("GOOD_name".into(), "a=b".into())],
        );
        assert_eq!(
            parse_environment(&["9bad=value".into()])
                .expect_err("invalid")
                .detail,
            "--env needs NAME=VALUE with a valid environment variable name",
        );
    }

    #[test]
    fn job_identity_contracts_are_not_path_names() {
        assert!(canonical_job_id("job-0123456789abcdef01234567"));
        assert!(!canonical_job_id("job-0123"));
        assert!(safe_job_identifier("job-legacy_1"));
        assert!(!safe_job_identifier("../job"));
    }
    #[test]
    fn retained_paths_and_byk_worker_paths_refuse_parent_escape() {
        let root = Path::new("/tmp/probierz-protected");
        assert!(safe_child(root, "../../escape", "unsafe").is_err());
        assert!(protected_byk_child(root, "/tmp/escape", "work directory").is_err());
        assert_eq!(
            protected_byk_child(root, "/tmp/probierz-protected/work", "work directory")
                .expect("protected child"),
            root.join("work"),
        );
    }

    #[test]
    fn byk_email_validation_requires_a_nonempty_dotted_domain() {
        assert!(valid_email("operator@example.com"));
        assert!(!valid_email("operator@example"));
        assert!(!valid_email("@example.com"));
        assert!(!valid_email("operator @example.com"));
    }

    #[test]
    fn remote_worker_bootstraps_the_locked_rust_and_node_products() {
        let script = run_script(
            "tui",
            "stado",
            None,
            None,
            "fixed",
            Some("linux"),
            "run",
            None,
            None,
            false,
            &[],
        )
        .expect("remote script");
        assert!(script.contains("cargo build --locked --release"));
        assert!(script.contains("node-v22.20.0-linux-x64.tar.xz"));
        assert!(script.contains("npm ci --no-audit --no-fund --loglevel=error"));
        assert!(script.contains("\"$PROBIERZ\" --harness \"$HARNESS\" run tui"));
        assert!(!script.contains("node agent/"));
    }

    #[test]
    fn source_snapshot_contains_product_manifests_but_not_runtime_results() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repository root");
        let list = source_file_list(root).expect("source list");
        let files: Vec<_> = list
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
            .map(|entry| String::from_utf8_lossy(entry).into_owned())
            .collect();
        assert!(files.iter().any(|entry| entry == "package.json"));
        assert!(files.iter().any(|entry| entry == "probierz-rs/Cargo.toml"));
        assert!(!files.iter().any(|entry| entry.starts_with("test-results/")));
    }
}
