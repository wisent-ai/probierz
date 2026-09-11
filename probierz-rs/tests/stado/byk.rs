//! Remote Byk: the selector refusal with Stado's own diagnostic, and the transport
//! built from the resolved host.

use crate::fixture::*;
use crate::*;

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
    assert!(forward[5]
        .as_str()
        .is_some_and(|port| port.parse::<u16>().is_ok()));
    assert_eq!(forward[6], "--local-port");
    assert!(forward[7]
        .as_str()
        .is_some_and(|port| port.parse::<u16>().is_ok()));
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
    let destination = source_delivery[4].as_str().expect("source destination");
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
    let bridge_token = worker_input["bridgeToken"].as_str().expect("bridge token");
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
