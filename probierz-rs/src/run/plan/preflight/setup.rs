use serde_json::json;
use crate::run::*;
pub(crate) fn setup_steps(harness: &Path, name: &str) -> Result<Vec<SetupStep>, Failure> {
    let npm = SetupStep {
        name: "npm install (workspaces)".into(),
        command: "npm".into(),
        args: vec!["install".into()],
        output_dir: None,
        skip_driver: None,
        skip_cua: false,
    };
    let pw = |pkg: &str, with_deps: bool| SetupStep {
        name: format!("playwright browsers ({pkg})"),
        command: "npm".into(),
        args: [
            "--workspace",
            &format!("packages/{pkg}"),
            "exec",
            "playwright",
            "install",
        ]
        .into_iter()
        .map(str::to_string)
        .chain(with_deps.then_some("--with-deps".into()))
        .collect(),
        output_dir: None,
        skip_driver: None,
        skip_cua: false,
    };
    let driver = |name: &str, version: Option<&str>| SetupStep {
        name: format!("appium driver: {name}"),
        command: "npx".into(),
        args: vec![
            "--no-install".into(),
            "appium".into(),
            "driver".into(),
            "install".into(),
            version
                .map(|version| format!("{name}@{version}"))
                .unwrap_or_else(|| name.into()),
        ],
        output_dir: None,
        skip_driver: Some(name.into()),
        skip_cua: false,
    };
    let native_binary = harness.join("node_modules/.cache/probierz/screen-capture-kit");
    let native = SetupStep {
        name: "ScreenCaptureKit recorder".into(),
        command: "xcrun".into(),
        args: vec![
            "swiftc".into(),
            "-parse-as-library".into(),
            harness
                .join("packages/desktop-native/tools/screen-capture-kit.swift")
                .to_string_lossy()
                .into_owned(),
            "-o".into(),
            native_binary.to_string_lossy().into_owned(),
        ],
        output_dir: native_binary.parent().map(Path::to_path_buf),
        skip_driver: None,
        skip_cua: false,
    };
    Ok(match name {
        "web" => vec![npm, pw("web", true)],
        "electron" => vec![npm, pw("electron", false)],
        "mobile:ios" => vec![npm, driver("xcuitest", None)],
        "mobile:android" => vec![npm, driver("uiautomator2", None)],
        "desktop:mac" => vec![npm, driver("mac2", Some("2.2.2")), native],
        "desktop:win" => vec![npm, driver("windows", None)],
        // The daemon this surface needs is started by the driver itself, in
        // `ensure_daemon`, so setup has nothing to install for it beyond the
        // workspace dependencies its report tooling shares.
        "desktop:cua" => vec![npm],
        "tui" => vec![npm],
        _ => {
            return Err(fail(
                "run.setup",
                format!("unknown target: {name} ({})", accepted_preflight_targets()),
            ))
        }
    })
}

pub fn setup(harness: &Path, name: &str, args: &[String]) -> Answer {
    if target(name).is_none() {
        return Err(fail("cli.setup", format!("unknown target: {name}")));
    }
    let opts = parse_run_args(args, false)?;
    let env = env_snapshot(&BTreeMap::new());
    let mut done = Vec::new();
    let mut failure: Option<(String, String)> = None;
    for step in setup_steps(harness, name)? {
        let line = format!("{} {}", step.command, step.args.join(" "));
        let skipped = step
            .skip_driver
            .as_ref()
            .is_some_and(|driver| appium_driver_installed(driver, &env))
            || (step.skip_cua && cua_accessibility_granted());
        if skipped {
            done.push(json!({ "step": step.name, "command": line, "ok": true, "skipped": true }));
            continue;
        }
        if let Some(directory) = &step.output_dir {
            fs::create_dir_all(directory)?;
        }
        let result = capture(
            &step.command,
            &step.args,
            Some(harness),
            None,
            Some(if opts.timeout_ms > 0 {
                opts.timeout_ms
            } else {
                30 * 60 * 1000
            }),
        );
        let ok = result.status.is_some_and(|status| status.success()) && !result.timed_out;
        let exit_code = result.status.and_then(|status| status.code()).unwrap_or(-1);
        done.push(json!({ "step": step.name, "command": line, "ok": ok, "exitCode": exit_code }));
        if !ok {
            let tail = tail_chars(&text(&result.stderr), 2000);
            failure = Some((step.name, tail));
            break;
        }
    }
    let ok = failure.is_none();
    let mut result = if let Some((step, stderr)) = failure {
        json!({ "target": name, "ok": false, "steps": done, "failedAt": step, "stderrTail": stderr })
    } else {
        json!({ "target": name, "ok": true, "steps": done })
    };
    result.as_object_mut().expect("object").insert(
        "preflight".into(),
        preflight(harness, name, &BTreeMap::new())?,
    );
    print_json(&result)?;
    if !ok {
        std::process::exit(1);
    }
    Ok(())
}

