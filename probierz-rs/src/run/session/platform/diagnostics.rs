use serde_json::json;
use crate::run::*;
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
    let elapsed = DateTime::parse_from_rfc3339(started)
        .map(|date| {
            ((Utc::now().timestamp_millis() - date.timestamp_millis()) as f64 / 1000.0).ceil()
                as i64
                + 5
        })
        .unwrap_or(60)
        .max(1);
    let (command, args, file) =
        if matches!(target, "desktop:mac" | "desktop:cua") && process_name.is_some() {
            let name = process_name.expect("checked");
            (
                "/usr/bin/log",
                vec![
                    "show".into(),
                    "--style".into(),
                    "compact".into(),
                    "--last".into(),
                    format!("{elapsed}s"),
                    "--predicate".into(),
                    format!("process == \"{name}\""),
                ],
                directory.join("macos-unified.log"),
            )
        } else if target == "mobile:ios" && process_name.is_some() {
            let name = process_name.expect("checked");
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
                    "--last".into(),
                    format!("{elapsed}s"),
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
    let result = capture(command, &args, None, None, Some(20_000));
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
        Value::String(tail_chars(detail, 2000))
    };
    json!({
        "supported": true,
        "file": file,
        "ok": ok,
        "exitCode": result.status.and_then(|status| status.code()).unwrap_or(-1),
        "error": failure,
    })
}

pub(crate) fn named_process_sample(process_name: Option<&str>) -> Option<Value> {
    let process_name = process_name?;
    if cfg!(windows) {
        return None;
    }
    let result = capture_text("ps", &["-axo", "comm=,rss=,%cpu="], None, Some(3000));
    if !result.status.is_some_and(|status| status.success()) {
        return None;
    }
    let expression = Regex::new(r"^(.*?)\s+(\d+)\s+([\d.]+)$").expect("regex");
    let mut rss_kb = 0.0;
    let mut cpu_percent = 0.0;
    let mut processes = 0;
    for line in text(&result.stdout).lines() {
        let Some(parts) = expression.captures(line.trim()) else {
            continue;
        };
        if Path::new(&parts[1])
            .file_name()
            .and_then(|name| name.to_str())
            != Some(process_name)
        {
            continue;
        }
        rss_kb += parts[2].parse::<f64>().unwrap_or(0.0);
        cpu_percent += parts[3].parse::<f64>().unwrap_or(0.0);
        processes += 1;
    }
    (processes > 0).then(|| {
        json!({
            "processes": processes,
            "rssKb": number(rss_kb),
            "cpuPercent": number(cpu_percent),
        })
    })
}

pub(crate) fn performance_sample(pgid: u32, process_name: Option<&str>) -> Option<Value> {
    if cfg!(windows) {
        return None;
    }
    let result = capture_text("ps", &["-axo", "pgid=,rss=,%cpu="], None, Some(3000));
    if !result.status.is_some_and(|status| status.success()) {
        return None;
    }
    let mut rss_kb = 0.0;
    let mut cpu_percent = 0.0;
    let mut processes = 0;
    for line in text(&result.stdout).lines() {
        let values: Vec<&str> = line.split_whitespace().collect();
        if values.first().and_then(|value| value.parse::<u32>().ok()) != Some(pgid) {
            continue;
        }
        rss_kb += values
            .get(1)
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0);
        cpu_percent += values
            .get(2)
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0);
        processes += 1;
    }
    Some(json!({
        "at": now_iso(),
        "processes": processes,
        "rssKb": number(rss_kb),
        "cpuPercent": number(cpu_percent),
        "app": named_process_sample(process_name),
    }))
}

