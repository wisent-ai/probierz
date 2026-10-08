use crate::run::*;
use serde_json::json;
pub(crate) fn collect_platform_diagnostics(
    target: &str,
    env: &BTreeMap<String, String>,
    artifacts: &Path,
    started: &str,
) -> Value {
    let directory = artifacts.join("diagnostics");
    let _ = fs::create_dir_all(&directory);
    let app_path = if matches!(target, "desktop:mac" | "desktop:cua") {
        env.get("MAC_APP_PATH")
    } else {
        env.get("APP_IOS")
    };
    let process_name = app_path
        .and_then(|path| Path::new(path).file_stem())
        .and_then(|name| name.to_str())
        .map(str::to_string);
    // The log window opens at the run's recorded start (`log show --start`),
    // so nothing is padded and nothing guessed; a start that does not parse
    // is reported instead of read over an invented span.
    let since = DateTime::parse_from_rfc3339(started)
        .map(|date| date.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .map_err(|error| format!("run start {started:?} is not RFC 3339 ({error}), so no log window can be read"));
    let unreadable_start = |error: &String| json!({ "supported": true, "file": null, "ok": false, "error": error });
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
    let _ = write_secure_text(&file, &output);
    let ok = result.status.is_some_and(|status| status.success());
    let failure = if ok {
        Value::Null
    } else {
        let detail = if error.trim().is_empty() {
            result.error.as_deref().unwrap_or("collector failed")
        } else {
            error.trim()
        };
        Value::String(detail.to_string())
    };
    json!({
        "supported": true,
        "file": file,
        "ok": ok,
        "exitCode": result.status.and_then(|status| status.code()).unwrap_or(-1),
        "error": failure,
    })
}
