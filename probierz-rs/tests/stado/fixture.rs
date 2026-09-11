//! The fixture that stands in for Stado's host inventory and the remote host it resolves.

use crate::*;

pub(crate) struct BykFixture {
    pub(crate) temporary: TempDir,
    pub(crate) harness: PathBuf,
    pub(crate) app: PathBuf,
    pub(crate) bin: PathBuf,
    pub(crate) broker: PathBuf,
    pub(crate) identity: PathBuf,
    pub(crate) capture: PathBuf,
}

impl BykFixture {
    pub(crate) fn new() -> Self {
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

    pub(crate) fn run(&self, selector: &str) -> Output {
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

pub(crate) fn write_executable(path: &Path, source: &str) {
    fs::write(path, source).expect("write executable");
    let mut permissions = fs::metadata(path)
        .expect("executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make executable");
}

pub(crate) fn json_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("capture")
        .lines()
        .map(|line| serde_json::from_str(line).expect("captured JSON"))
        .collect()
}
