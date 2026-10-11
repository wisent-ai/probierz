use crate::run::*;
use serde_json::json;
pub(crate) fn collect_platform_diagnostics(
    target: &str,
    env: &BTreeMap<String, String>,
    artifacts: &Path,
    started: &str,
) -> Value {
    let directory = artifacts.join("diagnostics");
    if let Err(error) = fs::create_dir_all(&directory) {
        return json!({
            "supported": true,
            "file": null,
            "ok": false,
            "error": format!("create platform diagnostics directory {}: {error}", directory.display()),
        });
    }
    let app_path = if matches!(target, "desktop:mac" | "desktop:cua") {
        env.get("MAC_APP_PATH")
    } else {
        env.get("APP_IOS")
    };
    let process_name = app_path
        .and_then(|path| Path::new(path).file_stem())
        .and_then(|name| name.to_str())
        .map(str::to_string);
    if matches!(target, "desktop:mac" | "desktop:cua" | "mobile:ios") && process_name.is_none() {
        let field = if target == "mobile:ios" {
            "APP_IOS"
        } else {
            "MAC_APP_PATH"
        };
        let error = match app_path {
            Some(path) => format!(
                "platform diagnostics for {target} cannot derive a process name from {field}={path:?}"
            ),
            None => format!(
                "platform diagnostics for {target} require {field} in the run environment"
            ),
        };
        return json!({ "supported": true, "file": null, "ok": false, "error": error });
    }
    // The log window opens at the run's recorded start (`log show --start`),
    // so nothing is padded and nothing guessed; a start that does not parse
    // is reported instead of read over an invented span.
    let since = DateTime::parse_from_rfc3339(started)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .map_err(|error| {
            format!("run start {started:?} is not RFC 3339 ({error}), so no log window can be read")
        });
    let unreadable_start =
        |error: &String| json!({ "supported": true, "file": null, "ok": false, "error": error });
    let (command, args, file) =
        if matches!(target, "desktop:mac" | "desktop:cua") && process_name.is_some() {
            let name = process_name.expect("checked");
            let since = match &since {
                Ok(since) => since.clone(),
                Err(error) => return unreadable_start(error),
            };
            (
                "/usr/bin/log",
                vec![
                    "show".into(),
                    "--style".into(),
                    "compact".into(),
                    "--start".into(),
                    since,
                    "--predicate".into(),
                    format!("process == \"{name}\""),
                ],
                directory.join("macos-unified.log"),
            )
        } else if target == "mobile:ios" && process_name.is_some() {
            let name = process_name.expect("checked");
            let since = match &since {
                Ok(since) => since.clone(),
                Err(error) => return unreadable_start(error),
            };
            (
                "xcrun",
                vec![
                    "simctl".into(),
                    "spawn".into(),
                    simulator_identifier(env.get("IOS_DEVICE").map(String::as_str)),
                    "log".into(),
                    "show".into(),
                    "--style".into(),
                    "compact".into(),
                    "--start".into(),
                    since,
                    "--predicate".into(),
                    format!("process == \"{name}\""),
                ],
                directory.join("ios-simulator.log"),
            )
        } else if target == "mobile:android" {
            let mut args = Vec::new();
            if let Some(device) = env.get("ANDROID_DEVICE") {
                args.extend(["-s".into(), device.clone()]);
            }
            args.extend([
                "logcat".into(),
                "-d".into(),
                "-v".into(),
                "threadtime".into(),
            ]);
            ("adb", args, directory.join("android-logcat.log"))
        } else {
            return json!({ "supported": false, "file": null });
        };
    let result = capture(command, &args, None, None);
    let output = redact_diagnostic(&text(&result.stdout));
    let error = redact_diagnostic(&text(&result.stderr));
    if let Err(write_error) = write_secure_text(&file, &output) {
        return json!({
            "supported": true,
            "file": null,
            "attemptedFile": file,
            "ok": false,
            "error": format!("retain platform diagnostic output at {}: {write_error}", file.display()),
            "collector": {
                "command": command,
                "args": args,
                "exitCode": result.status.and_then(|status| status.code()),
                "error": result.error,
                "stderr": error,
            },
        });
    }
    let ok = result.error.is_none() && result.status.is_some_and(|status| status.success());
    let failure = if ok {
        Value::Null
    } else {
        let mut causes = Vec::new();
        if let Some(capture_error) = &result.error {
            causes.push(redact_diagnostic(capture_error));
        }
        if !error.trim().is_empty() {
            causes.push(error.clone());
        }
        match result.status {
            Some(status) if !status.success() => {
                causes.push(format!("{command} {}", crate::failure::ended(&status)))
            }
            None => causes.push(format!("{command} has no observed exit status")),
            _ => {}
        }
        Value::String(causes.join("; "))
    };
    json!({
        "supported": true,
        "file": file,
        "ok": ok,
        "exitCode": result.status.and_then(|status| status.code()),
        "error": failure,
    })
}
