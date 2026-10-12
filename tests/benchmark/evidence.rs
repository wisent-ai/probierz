//! `probierz benchmark scout --evidence` and a cycle whose policy names an
//! evidence directory, through the real binary on a PATH that holds neither
//! Trends nor Competitors: the trend, the observations and the candidates
//! come from the file the person named, a file for another topic or without
//! a part is refused by name, a trend below its floor is refused with the
//! trend printed, a buildable trend reaches the model step (refused here for
//! the router nobody declared, which proves Trends was never asked), and the
//! cycle lists the directory's topics without a Trends on the machine. Every
//! figure in the evidence is measured from the observations the test lists;
//! the cycle's policy is the harness's own with the directory added.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");
const TOPIC: &str = "browser-agents";
/// The observations' titles, one line each.
const TITLES: &str = "An agent that browses for you\nBrowsing agents compared";

fn harness(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("benchmark-evidence")
        .join(name);
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove the previous run's harness");
    }
    std::fs::create_dir_all(root.join("apps")).expect("create the harness");
    std::fs::create_dir_all(root.join("empty-path")).expect("create the empty PATH");
    root
}

/// The binary with an empty PATH: no Trends, no Competitors, no Stado.
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(PROBIERZ)
        .args(args)
        .env("PROBIERZ_HARNESS_DIR", root)
        .env("PATH", root.join("empty-path"))
        .env_remove("STADO_MODEL_ROUTER_URL")
        .env_remove("STADO_MODEL_ROUTER_TOKEN")
        .output()
        .expect("start probierz")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// What Trends and Competitors would have answered for `topic`, every figure
/// measured from the listed observations: `buildable` sets the floor at the
/// count seen and the earlier window at nothing, so the trend holds; not
/// buildable sets the floor above what was seen.
fn evidence(topic: &str, buildable: bool) -> Value {
    let now = chrono::Utc::now().to_rfc3339();
    let observations: Vec<Value> = TITLES
        .lines()
        .enumerate()
        .map(|(index, title)| {
            json!({"id": format!("obs-{index}"), "title": title, "url": format!("https://example.invalid/{index}"), "published_at": now})
        })
        .collect();
    let seen = observations.len();
    let floor = if buildable { seen } else { seen + seen };
    let baseline = if buildable { seen - seen } else { seen };
    let verdict = if buildable {
        "rising"
    } else {
        "insufficient-evidence"
    };
    let first = observations.first().expect("one observation")["id"].clone();
    json!({
        "trend": {"topic": topic, "verdict": verdict, "evidence": seen, "min_evidence": floor, "recent": seen, "baseline": baseline},
        "observations": observations,
        "candidates": [
            {"id": "example-agent", "name": "Example Agent", "domains": ["example.invalid"], "description": "An agent that browses", "evidence": [{"attributes": {"observations": [first]}}]}
        ]
    })
}

fn write(path: &Path, document: &Value) {
    std::fs::write(
        path,
        serde_json::to_string_pretty(document).expect("encode") + "\n",
    )
    .expect("write the evidence");
}

#[test]
fn scout_reads_named_evidence_and_refuses_what_the_file_lacks() {
    let root = harness("scout");
    let file = root.join(format!("{TOPIC}.json"));
    let count = TITLES.lines().count().to_string();
    let scout = || {
        run(
            &root,
            &[
                "benchmark",
                "scout",
                TOPIC,
                "--owner",
                "wisent-ai",
                "--observations",
                &count,
                "--rounds",
                &count,
                "--evidence",
                file.to_str().expect("a path"),
            ],
        )
    };

    let missing = scout();
    assert!(!missing.status.success(), "{}", stderr(&missing));
    assert!(
        stderr(&missing).contains("cannot be read"),
        "{}",
        stderr(&missing)
    );

    write(&file, &evidence("other-topic", true));
    let other = scout();
    assert!(
        stderr(&other).contains("holds the trend of topic \"other-topic\", not browser-agents"),
        "{}",
        stderr(&other)
    );

    let mut partial = evidence(TOPIC, true);
    partial
        .as_object_mut()
        .expect("an object")
        .remove("observations");
    write(&file, &partial);
    let lacking = scout();
    assert!(
        stderr(&lacking).contains("has no observations"),
        "{}",
        stderr(&lacking)
    );

    write(&file, &evidence(TOPIC, false));
    let refused = scout();
    assert!(!refused.status.success(), "{}", stderr(&refused));
    let printed: Value =
        serde_json::from_slice(&refused.stdout).expect("the refusal prints the trend");
    assert_eq!(printed["trend"]["verdict"], "insufficient-evidence");
    assert!(
        stderr(&refused).contains("Trends reads topic browser-agents as insufficient-evidence"),
        "{}",
        stderr(&refused)
    );
    assert!(
        !root.join("test-results").join(".scout").exists(),
        "a refused scout wrote a brief"
    );

    write(&file, &evidence(TOPIC, true));
    let reached = scout();
    assert!(!reached.status.success(), "{}", stderr(&reached));
    let said = stderr(&reached);
    assert!(
        said.contains("STADO_MODEL_ROUTER_URL is required"),
        "the model step was not reached from the file's evidence: {said}"
    );
    assert!(
        !said.contains("trends"),
        "Trends was asked although the evidence was named: {said}"
    );
}

/// The harness's own policy with the evidence directory added.
fn policy_naming(directory: &str) -> String {
    let own =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../autonomy.yaml"))
            .expect("the harness's own policy");
    format!("{own}evidenceDir: {directory}\n")
}

/// The report the cycle wrote under the harness.
fn cycle_report(root: &Path, said: &str) -> Value {
    let report_dir = root.join("test-results").join(".autonomy");
    let found = std::fs::read_dir(&report_dir)
        .expect("the cycle wrote its report directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("cycle-"))
        });
    let Some(report) = found else {
        panic!("no cycle report under {}: {said}", report_dir.display());
    };
    serde_json::from_str(&std::fs::read_to_string(&report).expect("read the report"))
        .expect("the report is JSON")
}

#[test]
fn a_cycle_scouts_the_evidence_directory_without_trends() {
    let root = harness("cycle");
    let directory = root.join("evidence");
    std::fs::create_dir_all(&directory).expect("create the evidence directory");
    write(
        &directory.join(format!("{TOPIC}.json")),
        &evidence(TOPIC, true),
    );
    std::fs::write(root.join("autonomy.yaml"), policy_naming("evidence"))
        .expect("write the policy");

    let missing = root.join("missing.yaml");
    std::fs::write(&missing, policy_naming("nowhere")).expect("write the policy");
    let refused = run(
        &root,
        &[
            "benchmark",
            "cycle",
            "--policy",
            missing.to_str().expect("a path"),
        ],
    );
    assert!(!refused.status.success(), "{}", stderr(&refused));
    assert!(
        stderr(&refused).contains("is not a directory of <topic>.json evidence files"),
        "{}",
        stderr(&refused)
    );

    let cycled = run(&root, &["benchmark", "cycle"]);
    let report = cycle_report(&root, &stderr(&cycled));
    assert_eq!(
        report["topics"]["listed"]["evidenceDir"],
        directory.to_string_lossy()
    );
    assert_eq!(report["topics"]["listed"]["topics"], json!([TOPIC]));
    assert_eq!(report["topics"]["added"], json!([]));
    assert_eq!(
        report["ingest"]["skipped"],
        "the policy names an evidence directory"
    );
    let scouted = report["scouted"]
        .as_array()
        .expect("scouted topics")
        .first()
        .expect("one scouted topic")
        .clone();
    assert_eq!(scouted["topic"], TOPIC);
    let scout = scouted["scout"]["args"].to_string();
    assert!(
        scout.contains("--evidence") && scout.contains(&format!("{TOPIC}.json")),
        "{scouted}"
    );
}
