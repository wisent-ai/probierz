//! `probierz check|run suites/<checkout>` through the real binary, against a
//! real Git checkout made for the case under the package's own build
//! directory, whose release manifest declares two suites for this platform:
//! one that writes a report and passes, one that fails.
//!
//! `check` lists both and no run; `run` with one gate passes and keeps
//! run.json with the checkout's revision, the gate's exit status and its
//! copied report; `run` with every gate is refused naming the kept run, whose
//! record says which gate failed; `check` then names that run as the newest.
//! A gate nothing declares and a checkout without a manifest are refused by
//! name.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");
const PRODUCT: &str = "suites-fixture";

fn said(output: &Output) -> String {
    format!(
        "exit {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// This host in the release pipeline's platform words.
fn platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    format!("{os}-{arch}")
}

/// The gate record named `name` in a run record.
fn gate<'a>(record: &'a Value, name: &str) -> &'a Value {
    match record["gates"]
        .as_array()
        .and_then(|gates| gates.iter().find(|gate| gate["name"] == name))
    {
        Some(gate) => gate,
        None => panic!("the run record names no gate {name}: {record}"),
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("start git");
    assert!(output.status.success(), "git {args:?}: {}", said(&output));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The fixture's two suites: one writes the checkout's revision to a report
/// it declares and passes, the other asks Git for a ref nobody made and fails.
fn gates() -> Value {
    json!([
        {
            "kind": "test",
            "name": "reported",
            "argv": ["git", "log", "--output=report.txt", "--format=%H"],
            "reports": ["report.txt"],
        },
        {
            "kind": "test",
            "name": "failing",
            "argv": ["git", "rev-parse", "--verify", "a-ref-nobody-made"],
        },
    ])
}

struct Case {
    /// Removed with the case.
    _owned: tempfile::TempDir,
    root: PathBuf,
    harness: PathBuf,
    checkout: PathBuf,
}

impl Case {
    fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        std::fs::create_dir_all(&base).unwrap();
        let owned = tempfile::tempdir_in(&base).unwrap();
        let root = owned.path().to_path_buf();
        let harness = root.join("harness");
        let checkout = root.join("checkout");
        // A harness is a root holding apps/; the suites command reads none of
        // its apps, but the binary refuses a harness root without it.
        std::fs::create_dir_all(harness.join("apps")).unwrap();
        std::fs::create_dir_all(&checkout).unwrap();
        git(&checkout, &["init", "--quiet", "--initial-branch=main"]);
        // The fixture's commits carry a name and no address.
        git(&checkout, &["config", "user.name", "probierz journey"]);
        git(&checkout, &["config", "user.email", ""]);
        let manifest = json!({
            "product": PRODUCT,
            "platforms": { platform(): { "quality": gates() } },
        });
        std::fs::write(
            checkout.join(".wisent-release.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        git(&checkout, &["add", ".wisent-release.json"]);
        git(
            &checkout,
            &["commit", "--quiet", "--message", "declare the suites"],
        );
        Self {
            _owned: owned,
            root,
            harness,
            checkout,
        }
    }

    fn probierz(&self, args: &[&str]) -> Output {
        Command::new(PROBIERZ)
            .arg("--harness")
            .arg(&self.harness)
            .args(args)
            .output()
            .expect("start probierz")
    }

    fn target(&self) -> String {
        format!("suites/{}", self.checkout.display())
    }
}

#[test]
fn a_checkout_s_own_suites_are_run_and_kept() {
    let case = Case::new();
    let target = case.target();
    let revision = git(&case.checkout, &["rev-parse", "HEAD"]);

    let listed = case.probierz(&["check", &target]);
    assert!(listed.status.success(), "{}", said(&listed));
    let listed: Value = serde_json::from_slice(&listed.stdout).unwrap();
    let names: Vec<&str> = listed["declared"]["suites"]
        .as_array()
        .unwrap()
        .iter()
        .map(|suite| suite["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["reported", "failing"]);
    assert_eq!(listed["lastRun"], Value::Null, "a run before any ran");

    let passed = case.probierz(&["run", &target, "reported"]);
    assert!(passed.status.success(), "{}", said(&passed));
    let passed: Value = serde_json::from_slice(&passed.stdout).unwrap();
    assert_eq!(passed["source"]["revision"], revision.as_str());
    assert_eq!(passed["verdict"], "passed");
    let reported = gate(&passed, "reported");
    assert_eq!(reported["passed"], true);
    let report = reported["reports"]
        .as_array()
        .and_then(|reports| reports.first())
        .and_then(|report| report["copied"].as_str())
        .unwrap();
    assert_eq!(std::fs::read_to_string(report).unwrap().trim(), revision);

    let failed = case.probierz(&["run", &target]);
    assert!(!failed.status.success(), "{}", said(&failed));
    let record: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(record["verdict"], "failed");
    assert_eq!(gate(&record, "reported")["passed"], true);
    assert_eq!(gate(&record, "failing")["passed"], false);
    let text = said(&failed);
    assert!(text.contains("suites failed; the run is kept in"), "{text}");
    assert!(
        text.contains(record["directory"].as_str().unwrap()),
        "{text}"
    );

    let after = case.probierz(&["check", &target]);
    assert!(after.status.success(), "{}", said(&after));
    let after: Value = serde_json::from_slice(&after.stdout).unwrap();
    assert_eq!(after["lastRun"]["directory"], record["directory"]);

    let unknown = case.probierz(&["run", &target, "undeclared"]);
    assert!(!unknown.status.success());
    assert!(
        said(&unknown).contains(r#"no test gate named \"undeclared\"; declared: reported, failing"#),
        "{}",
        said(&unknown)
    );

    let bare = case.root.join("no-manifest");
    std::fs::create_dir_all(&bare).unwrap();
    let missing = case.probierz(&["check", &format!("suites/{}", bare.display())]);
    assert!(!missing.status.success());
    assert!(
        said(&missing).contains(".wisent-release.json could not be read"),
        "{}",
        said(&missing)
    );
    for (quality, cause) in [
        (
            json!([{"kind": "test", "name": "../escaped", "argv": ["git", "status"]}]),
            "must have a single path-safe name",
        ),
        (
            json!([{"kind": "test", "name": "escaped", "argv": ["git", "status"],
                "reports": ["../escaped"]}]),
            "must be checkout-relative without parent traversal",
        ),
        (
            json!([
                {"kind": "test", "name": "duplicate", "argv": ["git", "status"]},
                {"kind": "test", "name": "duplicate", "argv": ["git", "status"]},
            ]),
            "is declared more than once",
        ),
    ] {
        let manifest = json!({
            "product": PRODUCT,
            "platforms": { platform(): { "quality": quality } },
        });
        std::fs::write(
            case.checkout.join(".wisent-release.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let refused = case.probierz(&["run", &target]);
        assert!(!refused.status.success(), "{}", said(&refused));
        assert!(said(&refused).contains(cause), "{}", said(&refused));
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_report_refuses_collection_but_keeps_the_run() {
    use std::os::unix::net::UnixListener;

    let case = Case::new();
    let manifest = json!({
        "product": PRODUCT,
        "platforms": { platform(): { "quality": [{
            "kind": "test",
            "name": "socket-report",
            "argv": ["git", "status", "--short"],
            "reports": ["report.sock"],
        }, {
            "kind": "test",
            "name": "following-gate",
            "argv": ["git", "rev-parse", "HEAD"],
        }] } },
    });
    std::fs::write(
        case.checkout.join(".wisent-release.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    git(&case.checkout, &["add", ".wisent-release.json"]);
    git(&case.checkout, &["commit", "--quiet", "--message", "declare socket report"]);
    // A socket's path must fit sockaddr_un (SUN_LEN), which the case's
    // absolute path below the package does not; cargo runs the test from the
    // package root, so the case is bound by its path relative to there.
    let socket = case.root.join("s");
    let here = std::env::current_dir().unwrap();
    let short = socket
        .strip_prefix(&here)
        .unwrap_or_else(|_| panic!("{} is not below the test's directory {}", socket.display(), here.display()));
    let _socket = UnixListener::bind(short).unwrap();
    std::os::unix::fs::symlink(case.root.join("s"), case.checkout.join("report.sock")).unwrap();

    let run = case.probierz(&["run", &case.target()]);
    assert!(!run.status.success(), "{}", said(&run));
    let record: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(record["verdict"], "failed");
    let outcome = gate(&record, "socket-report");
    assert!(outcome["exitCode"].is_number(), "the suite itself completed");
    assert_eq!(outcome["passed"], false);
    let report = outcome["reports"].as_array().unwrap().first().unwrap();
    let destination = Path::new(record["directory"].as_str().unwrap())
        .join("socket-report/reports/report.sock");
    assert_eq!(report["destination"], destination.to_str().unwrap());
    assert!(report["error"].as_str().unwrap().contains("symbolic link"));
    assert_eq!(gate(&record, "following-gate")["passed"], true);
    assert!(said(&run).contains(record["directory"].as_str().unwrap()));

    let after = case.probierz(&["check", &case.target()]);
    assert!(after.status.success(), "{}", said(&after));
    let after: Value = serde_json::from_slice(&after.stdout).unwrap();
    assert_eq!(after["lastRun"]["directory"], record["directory"]);
    assert_eq!(gate(&after["lastRun"], "socket-report")["passed"], false);
    assert_eq!(gate(&after["lastRun"], "following-gate")["passed"], true);
}
