use crate::specs::tui::common::{self, Output, Service};
use std::{
    collections::BTreeMap,
    fs,
    net::TcpListener,
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub fn env_file(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = fs::read_to_string(path)
        .map_err(|_| format!("{} must identify a readable file", path.display()))?;
    let mut out = BTreeMap::new();
    for source in text.lines() {
        let line = source.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((raw_name, raw_value)) = line.split_once('=') else {
            continue;
        };
        let name = raw_name
            .trim()
            .strip_prefix("export ")
            .unwrap_or(raw_name.trim());
        let mut value = raw_value.trim().to_string();
        if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            value = value[1..value.len() - 1].to_string();
        }
        out.insert(name.to_string(), value);
    }
    Ok(out)
}

pub fn server_test(
    repository: &Path,
    endpoint: &str,
    test_script: &str,
    mut env: BTreeMap<String, String>,
    dist: Option<&str>,
    ready_name: &str,
    timeout: Duration,
) -> Result<(Output, String), String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);
    let base = format!("http://127.0.0.1:{port}");
    if let Some(dist) = dist {
        env.insert("ECHO_NEXT_DIST_DIR".into(), dist.into());
    }
    let args = common::strings(&[
        "run",
        "dev",
        "--",
        "--hostname",
        "127.0.0.1",
        "--port",
        &port.to_string(),
    ]);
    let mut service = Service::spawn("npm", &args, repository, &env)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if service.exited()? {
            return Err(format!(
                "{ready_name} exited before readiness:\n{}",
                service.log()
            ));
        }
        if ureq::get(&format!("{base}{endpoint}")).call().is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{ready_name} did not become ready:\n{}",
                service.log()
            ));
        }
        thread::sleep(Duration::from_millis(250));
    }
    let base_name = if test_script == "test:docs" {
        "ECHO_DOCS_BASE_URL"
    } else if test_script == "test:analytics" {
        "ECHO_ANALYTICS_TEST_BASE_URL"
    } else {
        "ECHO_GUI_TEST_BASE_URL"
    };
    env.insert(base_name.into(), base);
    let result = common::run(
        "npm",
        &common::strings(&["run", test_script]),
        Some(repository),
        &env,
        &[],
        None,
        timeout,
    )?;
    let logs = service.log();
    service.stop();
    Ok((result, logs))
}
