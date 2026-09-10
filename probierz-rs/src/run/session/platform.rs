use serde_json::json;
use crate::run::*;
use crate::run::report::identity::Mode600;
pub(crate) fn run_data_command(
    harness: &Path,
    config: Option<&serde_yaml::Value>,
    env: &BTreeMap<String, String>,
    secrets: &[(String, String)],
    stdout: &Path,
    stderr: &Path,
) -> Value {
    let Some(config) = config else {
        return json!({ "ok": true, "result": null });
    };
    let args = match config.get("args") {
        Some(value) => match value.as_sequence() {
            Some(values) => values
                .iter()
                .map(|value| yaml_string(value).unwrap_or_default())
                .collect::<Vec<_>>(),
            None => return json!({ "ok": false, "error": "invalid data command configuration" }),
        },
        None => Vec::new(),
    };
    if let Some(capability) = config.get("capability").and_then(serde_yaml::Value::as_str) {
        return match crate::apphooks::execute(harness, capability, &args, env) {
            Ok(result) => json!({ "ok": true, "result": result }),
            Err(error) => json!({ "ok": false, "error": error.detail }),
        };
    }
    let Some(command) = config.get("command").and_then(serde_yaml::Value::as_str) else {
        return json!({ "ok": false, "error": "invalid data command configuration" });
    };
    let cwd = config
        .get("cwd")
        .and_then(serde_yaml::Value::as_str)
        .map(|cwd| normalize_path(&harness.join(cwd)))
        .unwrap_or_else(|| harness.to_path_buf());
    let timeout = config
        .get("timeoutMs")
        .and_then(serde_yaml::Value::as_u64)
        .filter(|value| *value > 0)
        .unwrap_or(120_000);
    let execution = capture(command, &args, Some(&cwd), Some(env), Some(timeout));
    let safe_out = redact_text(&text(&execution.stdout), secrets);
    let safe_err = redact_text(&text(&execution.stderr), secrets);
    if !safe_out.is_empty() {
        let _ = append_secure(stdout, stamped(&safe_out).as_bytes());
    }
    if !safe_err.is_empty() {
        let _ = append_secure(stderr, stamped(&safe_err).as_bytes());
    }
    if execution.error.is_some() || !execution.status.is_some_and(|status| status.success()) {
        let detail = execution.error.unwrap_or_else(|| {
            let trimmed = safe_err.trim();
            if trimmed.is_empty() {
                format!(
                    "exit {}",
                    execution
                        .status
                        .and_then(|status| status.code())
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "null".into())
                )
            } else {
                trimmed.into()
            }
        });
        return json!({ "ok": false, "error": tail_chars(&detail, TAIL), "exitCode": execution.status.and_then(|status| status.code()) });
    }
    let stdout_text = text(&execution.stdout);
    let lines: Vec<&str> = stdout_text
        .lines()
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return json!({ "ok": true, "result": null });
    }
    match serde_json::from_str::<Value>(lines.last().expect("not empty")) {
        Ok(result) => json!({ "ok": true, "result": result }),
        Err(_) => json!({ "ok": false, "error": "data command did not return JSON" }),
    }
}
pub(crate) fn append_secure(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).append(true).mode_600();
    options.open(path)?.write_all(bytes)
}

pub(crate) fn report_identity(path: &Path, run_id: &str, started: SystemTime) -> Value {
    if !path.exists() {
        return json!({ "ok": false, "error": "report missing" });
    }
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) => return json!({ "ok": false, "error": format!("report unreadable: {error}") }),
    };
    if meta
        .modified()
        .ok()
        .is_some_and(|modified| modified < started)
    {
        return json!({ "ok": false, "error": "report predates run start" });
    }
    match fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    {
        Some(report) => {
            let actual = report.pointer("/probierz/runId").and_then(Value::as_str);
            if actual != Some(run_id) {
                json!({ "ok": false, "error": format!("report run ID mismatch: expected {run_id}, got {}", actual.unwrap_or("missing")) })
            } else {
                json!({ "ok": true, "runId": run_id, "mtime": meta.modified().map(system_time_iso).unwrap_or_else(|_| now_iso()) })
            }
        }
        None => json!({ "ok": false, "error": "report unreadable: invalid JSON" }),
    }
}

pub(crate) fn redact_diagnostic(value: &str) -> String {
    Regex::new(r"([?&][^=\s&]+)=([^&\s]+)")
        .expect("regex")
        .replace_all(&safe_message(value), "$1=[VALUE]")
        .into_owned()
}

pub(crate) fn simulator_identifier(requested: Option<&str>) -> String {
    let requested = requested.unwrap_or("booted");
    if requested == "booted" {
        return requested.into();
    }
    let document = simctl_devices();
    let candidates: Vec<&Value> = document
        .get("devices")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|devices| devices.values())
        .filter_map(Value::as_array)
        .flatten()
        .filter(|device| {
            device.get("udid").and_then(Value::as_str) == Some(requested)
                || device.get("name").and_then(Value::as_str) == Some(requested)
        })
        .collect();
    candidates
        .iter()
        .find(|device| device.get("state").and_then(Value::as_str) == Some("Booted"))
        .copied()
        .or_else(|| candidates.first().copied())
        .and_then(|device| device.get("udid").and_then(Value::as_str))
        .unwrap_or(requested)
        .to_string()
}

pub(crate) fn write_secure_text(path: &Path, value: &str) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true).mode_600();
    options.open(path)?.write_all(value.as_bytes())
}

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

