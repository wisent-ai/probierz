use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};

use serde_json::Value;
use tempfile::tempdir;

fn post(port: u16, token: Option<&str>, body: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to intake");
    let authorization = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    write!(
        stream,
        "POST /v1/failures HTTP/1.1\r\nHost: 127.0.0.1\r\n{authorization}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    )
    .expect("write request");
    stream.flush().expect("flush request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    let (headers, body) = response.split_once("\r\n\r\n").expect("HTTP response");
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .expect("HTTP status");
    (status, serde_json::from_str(body).expect("JSON response"))
}

#[test]
fn intake_accepts_a_real_envelope_with_its_bearer_and_refuses_without_it() {
    let root = tempdir().expect("temporary harness");
    fs::create_dir(root.path().join("apps")).expect("apps directory");
    let failures = root.path().join("failures");
    let token = "integration-secret";
    let reserved = TcpListener::bind(("127.0.0.1", 0)).expect("reserve port");
    let port = reserved.local_addr().expect("local address").port();
    drop(reserved);

    let mut child = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .args([
            "--harness",
            root.path().to_str().expect("UTF-8 harness path"),
            "intake",
            "serve",
            "--bind",
            &format!("127.0.0.1:{port}"),
        ])
        .env("PROBIERZ_INTAKE_TOKEN", token)
        .env("PROBIERZ_FAILURES_DIR", &failures)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start intake");
    let stderr = child.stderr.take().expect("intake stderr");
    let mut stderr = BufReader::new(stderr);
    let mut ready = String::new();
    stderr
        .read_line(&mut ready)
        .expect("read listener readiness");
    assert!(
        ready.contains(&format!("listening on http://127.0.0.1:{port}/v1/failures")),
        "{ready}"
    );

    let body = r#"{"failure_point":"desktop.window.load","error_code":"unknown","service":"Probierz Desktop","impact":"screen","detail":"window did not load","context":{"attempt":2}}"#;
    let (status, refused) = post(port, None, body);
    assert_eq!(status, 401);
    assert_eq!(refused["failure_point"], "probierz.intake.request");
    assert_eq!(refused["error_code"], "auth");
    assert_eq!(refused["detail"], "missing or wrong bearer token");

    let (status, accepted) = post(port, Some(token), body);
    assert_eq!(status, 202);
    assert_eq!(accepted, serde_json::json!({ "accepted": true }));

    let stored =
        fs::read_to_string(failures.join("probierz-desktop.jsonl")).expect("stored envelope");
    let stored: Value = serde_json::from_str(stored.trim()).expect("stored JSON line");
    assert_eq!(stored["failure_point"], "desktop.window.load");
    assert_eq!(stored["error_code"], "unknown");
    assert_eq!(stored["service"], "Probierz Desktop");
    assert_eq!(stored["context"]["attempt"], 2);
    assert!(stored["received_at"].as_str().is_some());

    child.kill().expect("stop intake");
    child.wait().expect("reap intake");

    let file_token = "token-from-file";
    let token_home = root.path().join("home");
    fs::create_dir_all(token_home.join(".probierz")).expect("token directory");
    fs::write(
        token_home.join(".probierz/intake-token"),
        format!("{file_token}\n"),
    )
    .expect("token file");
    let reserved = TcpListener::bind(("127.0.0.1", 0)).expect("reserve second port");
    let file_port = reserved.local_addr().expect("second local address").port();
    drop(reserved);
    let mut file_child = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .args([
            "--harness",
            root.path().to_str().expect("UTF-8 harness path"),
            "intake",
            "serve",
            "--bind",
            &format!("127.0.0.1:{file_port}"),
        ])
        .env_remove("PROBIERZ_INTAKE_TOKEN")
        .env("HOME", &token_home)
        .env(
            "PROBIERZ_FAILURES_DIR",
            root.path().join("file-token-failures"),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start intake with file token");
    let mut file_stderr = BufReader::new(file_child.stderr.take().expect("file-token stderr"));
    let mut file_ready = String::new();
    file_stderr
        .read_line(&mut file_ready)
        .expect("read file-token readiness");
    assert!(
        file_ready.contains(&format!(
            "listening on http://127.0.0.1:{file_port}/v1/failures"
        )),
        "{file_ready}"
    );
    assert_eq!(post(file_port, Some(file_token), body).0, 202);
    file_child.kill().expect("stop file-token intake");
    file_child.wait().expect("reap file-token intake");
}
