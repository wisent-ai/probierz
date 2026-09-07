use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use serde_json::{json, Value};
use tempfile::tempdir;

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(port: u16, method: &str, path: &str, body: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to local API");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    )
    .expect("write HTTP request");
    stream.flush().expect("flush HTTP request");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read HTTP response");
    parse_response(&response)
}

fn oversized_request(port: u16) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to local API");
    write!(
        stream,
        "POST /v1/project-adoptions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        1024 * 1024 + 1,
    )
    .expect("write oversized request headers");
    stream.flush().expect("flush oversized request headers");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read oversized response");
    parse_response(&response)
}

fn parse_response(response: &str) -> (u16, Value) {
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .expect("complete HTTP response");
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .expect("HTTP status");
    assert!(headers
        .to_ascii_lowercase()
        .contains("content-type: application/json; charset=utf-8"));
    assert!(headers
        .to_ascii_lowercase()
        .contains("cache-control: no-store"));
    assert!(body.ends_with('\n'));
    (status, serde_json::from_str(body).expect("JSON response"))
}

fn repository(root: &Path) {
    fs::create_dir_all(root.join(".git")).expect("Git marker");
}

fn adoption_source(root: &Path) {
    repository(root);
    fs::create_dir_all(root.join("apps/example")).expect("app directory");
    fs::create_dir_all(root.join("packages/tui/tests")).expect("spec directory");
    fs::write(
        root.join("apps/example/probierz.yaml"),
        format!(
            "schemaVersion: 1\nappId: example\nowner: example maintainers\nrepositories:\n  - root: {}\n    mappings: []\nsurfaces:\n  tui:\n    spec: example.spec.mjs\n    journeys: [smoke]\njourneys:\n  smoke:\n    owner: example maintainers\n    timeoutMs: 1000\n",
            root.display(),
        ),
    )
    .expect("manifest");
    fs::write(
        root.join("packages/tui/tests/example.spec.mjs"),
        "describe('example', () => { it('smoke', () => {}); });\n",
    )
    .expect("spec");
}

fn identical_adoption_source(root: &Path, source: &Path) {
    repository(root);
    for relative in [
        "apps/example/probierz.yaml",
        "packages/tui/tests/example.spec.mjs",
    ] {
        let target = root.join(relative);
        fs::create_dir_all(target.parent().expect("definition parent"))
            .expect("second source directory");
        fs::copy(source.join(relative), &target).expect("copy identical definition");
        #[cfg(unix)]
        {
            fs::set_permissions(&target, fs::metadata(source.join(relative)).unwrap().permissions())
                .expect("copy definition mode");
        }
    }
}

fn assert_serve_refusal(root: &Path, arguments: &[&str], sentence: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root)
        .args(arguments)
        .output()
        .expect("run refused serve command");
    assert_eq!(output.status.code(), Some(2), "{arguments:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(sentence),
        "{arguments:?} did not report {sentence:?}:\n{stderr}"
    );
}

#[test]
fn local_api_serves_every_route_and_preserves_its_refusals() {
    let destination = tempdir().expect("temporary destination");
    repository(destination.path());
    fs::create_dir(destination.path().join("apps")).expect("destination apps");
    let source = tempdir().expect("temporary source");
    adoption_source(source.path());

    let mut child = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .args([
            "--harness",
            destination.path().to_str().expect("UTF-8 destination"),
            "serve",
            "--port",
            "0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start local API");
    let stdout = child.stdout.take().expect("local API stdout");
    let mut stdout = BufReader::new(stdout);
    let mut ready_line = String::new();
    stdout
        .read_line(&mut ready_line)
        .expect("read local API readiness");
    let ready: Value = serde_json::from_str(&ready_line).expect("readiness JSON");
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["host"], "127.0.0.1");
    let port = ready["port"].as_u64().expect("listener port") as u16;
    let _server = Server(child);

    let (status, health) = request(port, "GET", "/v1/health", "");
    assert_eq!(status, 200);
    assert_eq!(health, json!({ "ok": true, "product": "probierz" }));

    let (status, empty) = request(port, "GET", "/v1/project-adoptions?desktop=1", "");
    assert_eq!(status, 200);
    assert_eq!(empty["schema"], "ai.wisent.probierz.project-adoptions.v1");
    assert_eq!(empty["sources"], json!([]));

    // The legacy Desktop API has no bearer scheme: loopback binding is its
    // access boundary, so an unauthenticated adoption is the documented call.
    let body = json!({
        "sourceRoot": source.path().to_string_lossy(),
        "replace": false,
    })
    .to_string();
    let (status, adopted) = request(port, "POST", "/v1/project-adoptions", &body);
    assert_eq!(status, 200);
    assert_eq!(adopted["status"], "imported");
    assert_eq!(adopted["applications"], json!(["example"]));
    assert_eq!(adopted["executedJourneys"], false);

    let (status, listed) = request(port, "GET", "/v1/project-adoptions", "");
    assert_eq!(status, 200);
    assert_eq!(listed["sources"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        listed["sources"][0]["sourceRoot"],
        source
            .path()
            .canonicalize()
            .expect("canonical source")
            .to_string_lossy()
            .as_ref(),
    );

    let other_source = tempdir().expect("second source");
    identical_adoption_source(other_source.path(), source.path());
    let body = json!({
        "sourceRoot": other_source.path().to_string_lossy(),
        "replace": true,
    })
    .to_string();
    let (status, conflict) = request(port, "POST", "/v1/project-adoptions", &body);
    assert_eq!(status, 200);
    assert_eq!(conflict["status"], "conflict");
    assert_eq!(conflict["conflicting"], 2);
    assert_eq!(conflict["rejected"], 2);
    assert!(conflict["conflicts"]
        .as_array()
        .expect("complete conflict list")
        .iter()
        .all(|item| item["reason"] == "destination is owned by another adopted source"));

    let (status, malformed) = request(port, "POST", "/v1/project-adoptions", "{");
    assert_eq!(status, 400);
    assert!(malformed["error"]
        .as_str()
        .is_some_and(|detail| !detail.is_empty()));

    let (status, missing) = request(port, "POST", "/v1/project-adoptions", r#"{"replace":true}"#);
    assert_eq!(status, 400);
    assert_eq!(missing, json!({ "error": "sourceRoot is required" }));

    let missing_source = json!({ "sourceRoot": "/a/path/that/does/not/exist" }).to_string();
    let (status, nonexistent) = request(port, "POST", "/v1/project-adoptions", &missing_source);
    assert_eq!(status, 400);
    assert!(nonexistent["error"]
        .as_str()
        .is_some_and(|detail| detail.contains("is not an existing directory")));

    let (status, oversized) = oversized_request(port);
    assert_eq!(status, 400);
    assert_eq!(oversized, json!({ "error": "request body exceeds 1 MiB" }));

    let (status, unknown) = request(port, "GET", "/v1/unknown", "");
    assert_eq!(status, 404);
    assert_eq!(unknown, json!({ "error": "not found" }));
}

#[test]
fn serve_help_and_refusals_match_the_documented_cli() {
    let root = tempdir().expect("temporary harness");
    fs::create_dir(root.path().join("apps")).expect("apps directory");

    let help = Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root.path())
        .args(["serve", "--help"])
        .output()
        .expect("read serve help");
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("--port <N>"), "{help}");
    assert!(help.contains("default: 0"), "{help}");

    for (arguments, sentence) in [
        (
            vec!["serve", "--port", "65536"],
            "--port needs an integer from 0 through 65535",
        ),
        (
            vec!["serve", "--port", "1.0"],
            "--port needs an integer from 0 through 65535",
        ),
        (
            vec!["serve", "--port"],
            "--port needs a number",
        ),
        (
            vec!["serve", "--unknown", "value"],
            "unknown serve option: --unknown",
        ),
        (
            vec!["serve", "--port=1"],
            "unknown serve option: --port=1",
        ),
        (
            vec!["serve", "--port", "1", "--port", "2"],
            "--port may be supplied only once",
        ),
    ] {
        assert_serve_refusal(root.path(), &arguments, sentence);
    }
}
