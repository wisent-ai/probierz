use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Stdio};
use std::sync::{Arc, Barrier};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(root: &Path) -> (Server, u16) {
    let mut process = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root)
        .args(["serve", "--port", "0"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(process.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    let ready: Value = serde_json::from_str(&ready).unwrap();
    (Server(process), ready["port"].as_u64().unwrap() as u16)
}

fn request(port: u16, action: &str, body: &Value) -> (u16, Value) {
    let body = body.to_string();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(stream, "POST /v1/incidents/{action} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    let (headers, body) = reply.split_once("\r\n\r\n").unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap())
}

#[test]
fn http_record_and_concurrent_resolutions_persist_exactly_one_closure() {
    let root = harness();
    let (_server, port) = start(root.path());
    let (status, refused) = request(
        port,
        "record",
        &serde_json::json!({"claim":"A claim", "envelope":{}}),
    );
    assert_eq!(status, 400);
    assert_eq!(
        refused["error"],
        "the envelope is not usable: failure_point must be a non-empty string"
    );
    assert!(!root
        .path()
        .join("test-results/.incidents/register.jsonl")
        .exists());
    let (status, incident) = request(
        port,
        "record",
        &serde_json::json!({
            "claim": "Verification finished", "run_id":"original-run",
            "envelope": {"service":"probierz", "failure_point":"verification.claim", "error_code":"invalid", "detail":"No run was retained", "context":{"revision":"source-revision"}}
        }),
    );
    assert_eq!(status, 200);
    let before = contents(root.path());
    let id = incident["incident_id"].as_str().unwrap().to_string();
    let barrier = Arc::new(Barrier::new(2));
    let requests: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|note| {
            let barrier = barrier.clone();
            let id = id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                request(
                    port,
                    "resolve",
                    &serde_json::json!({"id":id, "note":note, "run_id":"verified-run"}),
                )
            })
        })
        .collect();
    let results: Vec<_> = requests
        .into_iter()
        .map(|request| request.join().unwrap())
        .collect();
    assert_eq!(
        results.iter().filter(|(status, _)| *status == 200).count(),
        1
    );
    let refusal = &results.iter().find(|(status, _)| *status == 400).unwrap().1;
    assert!(refusal["error"]
        .as_str()
        .unwrap()
        .contains("was resolved at"));
    let after = contents(root.path());
    assert!(after.starts_with(&before));
    assert_eq!(lines(root.path()).len(), 2);
    assert_eq!(lines(root.path())[1]["run_id"], "verified-run");
    let (status, shown) = request(port, "show", &serde_json::json!({"id":id}));
    assert_eq!(status, 200);
    assert_eq!(shown["state"], "resolved");
    assert_eq!(shown["envelope"]["context"]["revision"], "source-revision");
    let (status, open) = request(
        port,
        "list",
        &serde_json::json!({"state":"open", "limit":20}),
    );
    assert_eq!(status, 200);
    assert_eq!(open["incidents"], serde_json::json!([]));
}
