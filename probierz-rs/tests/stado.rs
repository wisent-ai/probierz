#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::{TempDir, tempdir};

fn run(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_probierz"))
        .arg("--harness")
        .arg(root)
        .args(arguments)
        .output()
        .expect("run probierz")
}

fn refused(root: &Path, arguments: &[&str], sentence: &str) {
    let output = run(root, arguments);
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded: {arguments:?}"
    );
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(
        stderr.contains(sentence),
        "missing exact refusal {sentence:?} in:\n{stderr}",
    );
}

fn harness() -> TempDir {
    let directory = tempfile::tempdir().expect("temporary harness");
    std::fs::create_dir(directory.path().join("apps")).expect("apps");
    directory
}

#[test]
fn documented_required_input_refusals_are_exact() {
    let root = harness();
    refused(
        root.path(),
        &["stado", "run"],
        "stado run needs a target (e.g. tui)",
    );
    refused(
        root.path(),
        &["stado", "run", "tui"],
        "stado run needs --app <appId>",
    );
    refused(
        root.path(),
        &["stado", "author"],
        "stado author needs an app ID and a journey name",
    );
    refused(
        root.path(),
        &["stado", "author", "demo", "journey"],
        "stado author needs --target <t>",
    );
    refused(
        root.path(),
        &["stado", "author", "demo", "journey", "--target", "web"],
        "stado author needs --desc <journey goal>",
    );
    refused(root.path(), &["stado", "seo"], "stado seo needs an app ID");
    refused(
        root.path(),
        &["stado", "collect", "job-0123456789abcdef01234567"],
        "stado collect needs --app <appId>",
    );
}

#[test]
fn documented_identity_and_cancellation_refusals_are_exact() {
    let root = harness();
    refused(
        root.path(),
        &["stado", "collect", "not-a-job", "--app", "demo"],
        "Collection requires a canonical Stado job ID and a known Stado host.",
    );
    refused(
        root.path(),
        &["stado", "resume", "../job"],
        "Resuming remote evidence needs a valid existing Stado job ID.",
    );
    refused(
        root.path(),
        &["stado", "cancel", "job-0123456789abcdef01234567"],
        "stado cancel needs --host <host>",
    );
    refused(
        root.path(),
        &[
            "stado",
            "cancel",
            "job-0123456789abcdef01234567",
            "--host",
            "stado:any",
        ],
        "stado cancel needs --reason <reason>",
    );
}

#[test]
fn documented_run_option_refusals_are_exact() {
    let root = harness();
    refused(
        root.path(),
        &[
            "stado",
            "run",
            "tui",
            "--app",
            "demo",
            "--env",
            "9BAD=value",
        ],
        "--env needs NAME=VALUE with a valid environment variable name",
    );
    refused(
        root.path(),
        &[
            "stado",
            "run",
            "tui",
            "--app",
            "demo",
            "--script",
            "remote/run.sh",
        ],
        "--script requires --node-source (custom app jobs run from app sources)",
    );
    refused(
        root.path(),
        &["stado", "run", "tui", "--app", "demo", "--app-binary-path"],
        "--app-binary-path needs a value",
    );
    refused(
        root.path(),
        &[
            "stado",
            "run",
            "tui",
            "--app",
            "demo",
            "--app-binary-path",
            "/tmp/demo",
        ],
        "--app-binary-path requires --app-repo <path>",
    );
    refused(
        root.path(),
        &[
            "stado", "run", "tui", "--app", "demo", "--cargo-release", "--node-source",
        ],
        "remote application provisioning options are mutually exclusive: --cargo-release, --node-source",
    );
    refused(
        root.path(),
        &["stado", "run", "tui", "--app", "demo", "--binary", "demo"],
        "--binary and --cargo-manifest require --cargo-release",
    );
}

struct BykFixture {
    temporary: TempDir,
    harness: PathBuf,
    app: PathBuf,
    bin: PathBuf,
    broker: PathBuf,
    identity: PathBuf,
    capture: PathBuf,
}

impl BykFixture {
    fn new() -> Self {
        let temporary = tempfile::Builder::new()
            .prefix("probierz-byk-")
            .tempdir_in("/tmp")
            .expect("temporary Byk fixture");
        let harness = temporary.path().join("harness");
        let app = temporary.path().join("Fixture.app");
        let bin = temporary.path().join("bin");
        let capture = temporary.path().join("capture");
        let home = temporary.path().join("home");
        for directory in [&harness, &app, &bin, &capture, &home] {
            fs::create_dir_all(directory).expect("fixture directory");
        }
        fs::create_dir_all(harness.join("apps")).expect("apps directory");
        fs::write(harness.join("fixture.txt"), "fixture").expect("fixture source");
        let identity = temporary.path().join("source-identity.json");
        fs::write(
            &identity,
            format!(
                "{{\"schemaVersion\":1,\"appId\":\"probierz\",\"harness\":{{\"worktreeSha256\":\"{}\"}},\"app\":null}}\n",
                "0".repeat(64)
            ),
        )
        .expect("source identity");
        let broker = bin.join("mailbox-broker");
        write_executable(
            &broker,
            r#"#!/usr/bin/python3
import json, os, socket, sys
socket_path = sys.argv[sys.argv.index("--socket") + 1]
server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
server.bind(socket_path)
server.listen()
print(json.dumps({"status":"ready","mailbox":"byk-ios-login","socket_path":socket_path,"recipient":"fixture@example.com"}), flush=True)
while True:
    connection, _ = server.accept()
    connection.close()
"#,
        );
        write_executable(
            &bin.join("stado"),
            r#"#!/usr/bin/python3
import json, os, sys
args = sys.argv[1:]
stdin = sys.stdin.buffer.read().decode("utf-8")
capture = os.path.join(os.environ["BYK_CAPTURE"], "stado.jsonl")
with open(capture, "a") as output:
    output.write(json.dumps({"argv": args, "stdin": stdin}) + "\n")
if args[:2] == ["host", "inventory"]:
    target = args[2]
    if target == "fixture-missing":
        print("registry says no target named fixture-missing", file=sys.stderr)
        sys.exit(1)
    print(json.dumps({
        "target": target,
        "declared_release_platform": "darwin-arm64"
    }))
elif args[:2] == ["host", "config-show"]:
    print(json.dumps({"file": "/Users/runner/.config/stado/config.json"}))
else:
    print("{}")
"#,
        );
        write_executable(
            &bin.join("git"),
            "#!/usr/bin/python3\nimport sys\nsys.stdout.buffer.write(b'fixture.txt\\0')\n",
        );
        Self {
            temporary,
            harness,
            app,
            bin,
            broker,
            identity,
            capture,
        }
    }

    fn run(&self, selector: &str) -> Output {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_probierz"))
            .args([
                "--harness",
                self.harness.to_str().expect("UTF-8 harness"),
                "run",
                "mobile:ios:byk-auth",
                "--force",
                "--no-analyze",
                "--host",
                selector,
            ])
            .env("PATH", path)
            .env("TMPDIR", self.temporary.path())
            .env("HOME", self.temporary.path().join("home"))
            .env("APP_IOS", &self.app)
            .env("BYK_MAILBOX_BROKER", &self.broker)
            .env("BYK_CAPTURE", &self.capture)
            .env("PROBIERZ_SOURCE_IDENTITY", &self.identity)
            .env("PROBIERZ_REPAIR_SUPPRESS", "1")
            .output()
            .expect("run remote Byk transport")
    }
}

fn write_executable(path: &Path, source: &str) {
    fs::write(path, source).expect("write executable");
    let mut permissions = fs::metadata(path).expect("executable metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make executable");
}

fn json_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("capture")
        .lines()
        .map(|line| serde_json::from_str(line).expect("captured JSON"))
        .collect()
}

#[test]
fn remote_byk_refuses_the_selector_with_stados_resolution_diagnostic() {
    let fixture = BykFixture::new();
    let output = fixture.run("stado:fixture-missing");
    assert!(!output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).expect("run JSON");
    assert_eq!(
        result["stderrTail"],
        "byk auth runner: Stado could not resolve Byk host selector \"stado:fixture-missing\": registry says no target named fixture-missing\n"
    );
    assert_eq!(
        json_lines(&fixture.capture.join("stado.jsonl")),
        vec![serde_json::json!({
            "argv": ["host", "inventory", "fixture-missing", "--json"],
            "stdin": ""
        })]
    );
    assert!(!fixture.capture.join("ssh.jsonl").exists());
    assert!(!fixture.capture.join("rsync.jsonl").exists());
    assert!(!fixture
        .temporary
        .path()
        .join("home/Library/Caches/probierz/remote-hosts/byk-auth.json")
        .exists());
}

#[test]
fn remote_byk_builds_the_stado_transport_from_resolved_host_inventory() {
    let fixture = BykFixture::new();
    let output = fixture.run("stado:fixture-mini");
    assert!(!output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).expect("run JSON");
    assert_eq!(result["exitCode"], 0);
    assert_eq!(result["reportValidation"]["error"], "report missing");

    let stado = json_lines(&fixture.capture.join("stado.jsonl"));
    assert_eq!(stado.len(), 11);
    let argv = |index: usize| stado[index]["argv"].as_array().expect("argument vector");
    assert_eq!(
        argv(0),
        serde_json::json!(["host", "inventory", "fixture-mini", "--json"])
            .as_array()
            .unwrap()
    );
    assert_eq!(
        argv(1),
        serde_json::json!(["host", "config-show", "fixture-mini"])
            .as_array()
            .unwrap()
    );
    assert_eq!(
        argv(2),
        serde_json::json!(["host", "ping", "fixture-mini", "--json"])
            .as_array()
            .unwrap()
    );
    let forward = argv(3);
    assert_eq!(forward.len(), 9);
    assert_eq!(
        &forward[..3],
        &serde_json::json!(["host", "forward-local", "fixture-mini"])
            .as_array()
            .unwrap()[..]
    );
    assert_eq!(forward[4], "--remote-port");
    assert!(forward[5].as_str().is_some_and(|port| port.parse::<u16>().is_ok()));
    assert_eq!(forward[6], "--local-port");
    assert!(forward[7].as_str().is_some_and(|port| port.parse::<u16>().is_ok()));
    assert_eq!(forward[8], "--json");
    assert_eq!(
        argv(4),
        serde_json::json!([
            "host",
            "exec",
            "fixture-mini",
            "--",
            "mkdir",
            "-p",
            ".stado/work/runs"
        ])
        .as_array()
        .unwrap()
    );

    let source_delivery = argv(5);
    let destination = source_delivery[4]
        .as_str()
        .expect("source destination");
    let run_id = destination
        .strip_prefix(".stado/work/runs/")
        .and_then(|value| value.strip_suffix("/probierz"))
        .expect("managed run UUID");
    assert_eq!(
        source_delivery,
        serde_json::json!([
            "host",
            "deliver",
            "fixture-mini",
            fixture.harness.display().to_string(),
            format!(".stado/work/runs/{run_id}/probierz"),
            "--files-from",
            "-",
            "--json"
        ])
        .as_array()
        .unwrap()
    );
    assert_eq!(stado[5]["stdin"].as_str(), Some("fixture.txt\0"));
    assert_eq!(
        argv(6),
        serde_json::json!([
            "host",
            "deliver",
            "fixture-mini",
            fixture.app.display().to_string(),
            format!(".stado/work/runs/{run_id}/Byk.app"),
            "--json"
        ])
        .as_array()
        .unwrap()
    );
    let remote_root = format!("/Users/runner/.stado/work/runs/{run_id}");
    assert_eq!(
        argv(7),
        serde_json::json!([
            "host",
            "build",
            "fixture-mini",
            "--manifest-path",
            format!("{remote_root}/probierz/probierz-rs/Cargo.toml"),
            "--bin",
            "probierz",
            "--json"
        ])
        .as_array()
        .unwrap()
    );
    assert_eq!(
        argv(8),
        serde_json::json!([
            "host",
            "run-attached",
            "fixture-mini",
            "--program",
            format!("{remote_root}/probierz/probierz-rs/target/release/probierz"),
            "--arg",
            "stado",
            "--arg",
            "byk-auth-worker"
        ])
        .as_array()
        .unwrap()
    );
    let worker_input: Value =
        serde_json::from_str(stado[8]["stdin"].as_str().expect("worker stdin"))
            .expect("worker JSON");
    assert_eq!(worker_input["recipient"], "fixture@example.com");
    assert_eq!(worker_input["runRoot"], remote_root);
    assert!(worker_input["otpPort"].as_u64().is_some());
    let bridge_token = worker_input["bridgeToken"]
        .as_str()
        .expect("bridge token");
    assert_eq!(bridge_token.len(), 36);
    assert!(stado
        .iter()
        .all(|call| !call["argv"].to_string().contains(bridge_token)));
    assert!(stado
        .iter()
        .all(|call| !call["argv"].to_string().contains("fixture@example.com")));
    assert_eq!(
        argv(9),
        serde_json::json!([
            "host",
            "remove-run-directory",
            "fixture-mini",
            remote_root.clone(),
            "--json"
        ])
        .as_array()
        .unwrap()
    );
    assert_eq!(
        argv(10),
        serde_json::json!([
            "host",
            "forward-close",
            "fixture-mini",
            forward[3],
            "--json"
        ])
        .as_array()
        .unwrap()
    );
    assert!(!fixture.capture.join("ssh.jsonl").exists());
    assert!(!fixture.capture.join("rsync.jsonl").exists());
}
