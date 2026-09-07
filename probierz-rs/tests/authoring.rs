use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

#[test]
fn mcp_speaks_discovery_protocol_over_stdio() {
    let harness = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("harness root");
    let probierz = env!("CARGO_BIN_EXE_probierz");
    let mut child = Command::new(env!("CARGO_BIN_EXE_probierz-mcp"))
        .env("PROBIERZ_BIN", probierz)
        .env("PROBIERZ_HARNESS", harness)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start probierz-mcp");
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"ping"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"probierz_list_surfaces","arguments":{}}}),
    ];
    {
        let stdin = child.stdin.as_mut().expect("mcp stdin");
        for request in requests {
            writeln!(stdin, "{request}").expect("write JSON-RPC request");
        }
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("finish probierz-mcp");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .expect("UTF-8 replies")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON-RPC response"))
        .collect();
    assert_eq!(replies.len(), 4);
    assert_eq!(
        replies[0],
        json!({
            "jsonrpc":"2.0", "id":1,
            "result": {"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"probierz","version":"0.1.0"}}
        })
    );
    assert_eq!(replies[1], json!({"jsonrpc":"2.0","id":2,"result":{}}));
    assert_eq!(
        replies[2]
            .pointer("/result/tools")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(45)
    );

    let direct = Command::new(probierz)
        .arg("--harness")
        .arg(harness)
        .arg("list")
        .output()
        .expect("run discovery command");
    assert!(direct.status.success());
    let direct: Value = serde_json::from_slice(&direct.stdout).expect("discovery JSON");
    let expected_text = serde_json::to_string_pretty(&direct).expect("pretty discovery JSON");
    assert_eq!(
        replies[3]
            .pointer("/result/content/0/type")
            .and_then(Value::as_str),
        Some("text")
    );
    assert_eq!(
        replies[3]
            .pointer("/result/content/0/text")
            .and_then(Value::as_str),
        Some(expected_text.as_str())
    );
}

#[test]
fn mcp_ignores_notifications_and_reports_parse_errors() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_probierz-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start probierz-mcp");
    {
        let stdin = child.stdin.as_mut().expect("mcp stdin");
        writeln!(
            stdin,
            "{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}}"
        )
        .unwrap();
        writeln!(stdin, "not-json").unwrap();
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("finish probierz-mcp");
    let lines: Vec<&str> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(
        serde_json::from_str::<Value>(lines[0]).unwrap(),
        json!({
            "jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}
        })
    );
}

#[test]
fn repair_refuses_when_no_failed_run_is_recorded() {
    let harness = tempfile::tempdir().expect("temporary harness");
    std::fs::create_dir(harness.path().join("apps")).expect("apps directory");
    let output = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(harness.path())
        .args(["repair", "demo", "--dry-run"])
        .output()
        .expect("run repair command");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({
            "ok": false,
            "sourceRunId": null,
            "failure": {
                "failure_point": "repair.dispatch",
                "error_code": "not_found",
                "retryable": false,
                "detail": "no failed run recorded for demo",
                "message": "No failed run is recorded for demo."
            }
        })
    );
}
