//! `probierz repair` reaches the model service `STADO_MODEL_ROUTER_URL`
//! names, Brama or any other service answering `/v1/chat/completions`, with
//! the bearer `STADO_MODEL_ROUTER_TOKEN` carries; nothing in the path asks
//! Brama by name. Through the real binary, in a harness made for the case
//! under the package's build directory: a real manifest of this harness with
//! its repository pointed at a git checkout of the case's own, and one failed
//! run classified as a product failure.
//!
//! The service is declared on a host that resolves to nothing, so the
//! request is sent and refused by the service's own unreachability, named
//! with the URL, not by a Brama lookup. With no service named, the repair
//! asks the real Stado (`PROBIERZ_TEST_STADO`) in a home of its own: with no
//! alternative answering it is refused naming each, and with the real
//! endpoint `PROBIERZ_TEST_MODEL_SERVICE_URL` declared in Brama's place it is
//! sent there with that endpoint's model and key.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");
/// The harness application whose real manifest the case copies, its
/// repository root replaced by the case's own checkout. It must be a manifest
/// this harness commits and that names exactly one repository.
const TEMPLATE_APP: &str = "echo-web";
const APP: &str = "demo";
const TARGET: &str = "web";
const RUN: &str = "failed-sign-in";
/// A model service that is not Brama, on a name nothing resolves.
const MODEL_SERVICE: &str = "https://model-service.invalid";
const BEARER: &str = "case-bearer-for-model-service";

/// The harness this binary was built in.
fn real_harness() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the harness")
        .to_path_buf()
}

/// A file the case needs, or a panic naming it and why it could not be read.
fn read(file: &Path) -> String {
    match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) => panic!("{} cannot be read: {error}", file.display()),
    }
}

/// The template manifest as the case's: its id and its repository root.
fn manifest(product: &Path) -> String {
    let template = real_harness()
        .join("apps")
        .join(TEMPLATE_APP)
        .join("probierz.yaml");
    let text = read(&template);
    let mut root_replaced = false;
    let rewritten: Vec<String> = text
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("appId:") {
                format!("appId: {APP}")
            } else if line.trim_start().starts_with("- root:") {
                assert!(
                    !root_replaced,
                    "the template names one repository: {}",
                    template.display()
                );
                root_replaced = true;
                format!("  - root: {}", product.display())
            } else {
                line.to_owned()
            }
        })
        .collect();
    assert!(
        root_replaced,
        "the template names a repository root: {}",
        template.display()
    );
    format!("{}\n", rewritten.join("\n"))
}

fn harness(case: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "repair-model-service-{case}-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove the previous harness");
    }
    let product = root.join("product");
    std::fs::create_dir_all(&product).expect("create the product checkout");
    let status = Command::new("git")
        .arg("-C")
        .arg(&product)
        .args(["init", "--quiet"])
        .status()
        .expect("git runs");
    assert!(status.success(), "git init failed in {}", product.display());
    let app = root.join("apps").join(APP);
    std::fs::create_dir_all(&app).expect("create the app directory");
    std::fs::write(app.join("probierz.yaml"), manifest(&product)).expect("write the manifest");
    let run = root.join("test-results").join(APP).join(TARGET).join(RUN);
    std::fs::create_dir_all(&run).expect("create the run directory");
    let record = json!({
        "runId": RUN, "appId": APP, "target": TARGET, "status": "failed",
        "failureOrigin": "product", "startedAt": "2026-10-01T10:00:00Z",
        "appManifest": { "journeys": ["violations-engine"] },
        "failure": { "message": "the scan reported nothing for a planted violation" },
    });
    std::fs::write(run.join("run-manifest.json"), record.to_string())
        .expect("write the run manifest");
    root
}

fn repair(root: &Path, configure: impl FnOnce(&mut Command)) -> Output {
    let mut command = Command::new(PROBIERZ);
    command
        .arg("--harness")
        .arg(root)
        // One round: each case is decided by the first request's answer.
        .args(["repair", "--rounds", "1", APP])
        .env_remove("STADO_MODEL_ROUTER_URL")
        .env_remove("STADO_MODEL_ROUTER_TOKEN")
        .env_remove("PROBIERZ_MODEL_AGENT_ID")
        .env_remove("PROBIERZ_MODEL_AGENT_SECRET");
    configure(&mut command);
    command.output().expect("start probierz")
}

/// The answer the repair printed; a panic carrying both streams when it is
/// not JSON.
fn answer(output: &Output) -> Value {
    match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(error) => panic!(
            "the repair prints its answer as JSON: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

/// The failure detail the answer carries; a panic with the answer when it
/// carries none.
fn detail(result: &Value) -> String {
    match result["failure"]["detail"].as_str() {
        Some(detail) => detail.to_owned(),
        None => panic!("the answer carries no failure detail: {result}"),
    }
}

/// Whether the repair wrote a round's brief before asking the service.
fn a_brief_was_written(root: &Path) -> bool {
    let repairs = root
        .join("test-results")
        .join(APP)
        .join("repairs")
        .join(RUN);
    match std::fs::read_dir(&repairs) {
        Ok(entries) => entries.flatten().any(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("round-") && name.ends_with("-brief.txt")
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => panic!("{} cannot be listed: {error}", repairs.display()),
    }
}

#[test]
fn the_repair_is_sent_to_the_declared_model_service_and_refused_by_it_not_by_brama() {
    let root = harness("declared");
    let output = repair(&root, |command| {
        command
            .env("STADO_MODEL_ROUTER_URL", MODEL_SERVICE)
            .env("STADO_MODEL_ROUTER_TOKEN", BEARER);
    });
    let result = answer(&output);
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["sourceRunId"], RUN, "{result}");
    let detail = detail(&result);
    assert!(
        detail.contains("model-service.invalid"),
        "the refusal names the declared service: {result}"
    );
    assert!(
        !detail.to_lowercase().contains("brama"),
        "a service that is not Brama is not refused as Brama: {result}"
    );
    assert!(
        a_brief_was_written(&root),
        "the brief was written before the request"
    );
    std::fs::remove_dir_all(&root).expect("remove the harness");
}

/// A home of the case's own whose Stado registry places Brama nowhere, with
/// the real Stado `PROBIERZ_TEST_STADO` first on PATH: the command Probierz
/// asks for a route when none is named.
struct Stado {
    home: PathBuf,
    bin: PathBuf,
    path: std::ffi::OsString,
}

impl Stado {
    fn isolated(root: &Path) -> Self {
        let stado = PathBuf::from(
            std::env::var("PROBIERZ_TEST_STADO").expect("declare the real stado executable"),
        );
        let home = root.join("home");
        let bin = root.join("bin");
        for directory in [&home, &bin, &home.join("tmp"), &home.join("storage")] {
            std::fs::create_dir_all(directory).expect("create the case's home");
        }
        std::os::unix::fs::symlink(&stado, bin.join("stado")).expect("link the real stado");
        let mut path = bin.clone().into_os_string();
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        let stado = Self { home, bin, path };
        stado.run(&["config", "init"], "");
        stado
    }

    fn apply(&self, command: &mut Command) {
        command
            .env("HOME", &self.home)
            .env("TMPDIR", self.home.join("tmp"))
            .env("WC_STORAGE_BACKEND", "local")
            .env("WC_LOCAL_STORAGE_PATH", self.home.join("storage"))
            .env("PATH", &self.path);
        for name in [
            "STADO_INFERENCE_ENDPOINT_URL",
            "STADO_INFERENCE_ENDPOINT_MODEL",
            "STADO_INFERENCE_ENDPOINT_KEY_ROLE",
            "STADO_CREDENTIALS_STORE",
        ] {
            command.env_remove(name);
        }
    }

    fn run(&self, arguments: &[&str], input: &str) -> String {
        use std::io::Write;
        let mut command = Command::new(self.bin.join("stado"));
        self.apply(&mut command);
        let mut child = command
            .args(arguments)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("start stado");
        child
            .stdin
            .take()
            .expect("stado's stdin")
            .write_all(input.as_bytes())
            .expect("write stado's stdin");
        let output = child.wait_with_output().expect("wait for stado");
        assert!(
            output.status.success(),
            "stado {} refused: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

#[test]
fn a_repair_with_no_router_named_and_no_alternative_answering_is_refused_naming_each() {
    let root = harness("undeclared");
    let stado = Stado::isolated(&root);
    let output = repair(&root, |command| {
        stado.apply(command);
        command.env("STADO_MODEL_ROUTER_TOKEN", BEARER);
    });
    let result = answer(&output);
    assert_eq!(result["ok"], false, "{result}");
    let detail = detail(&result);
    for named in [
        "STADO_MODEL_ROUTER_URL is not set and no inference alternative answers probierz",
        "brama at ",
        "model-endpoint at capabilities.inference.endpoint.url",
    ] {
        assert!(detail.contains(named), "{named:?} is not named: {result}");
    }
    std::fs::remove_dir_all(&root).expect("remove the harness");
}

/// With no router named, the repair is sent to the real OpenAI-compatible
/// endpoint `PROBIERZ_TEST_MODEL_SERVICE_URL` Stado resolves in Brama's
/// place, asking its model `PROBIERZ_TEST_MODEL_SERVICE_MODEL` with the key
/// (from the owner-only file `PROBIERZ_TEST_MODEL_SERVICE_KEY_FILE`) its role
/// holds in the case's own credential store, and its answer is what decides.
#[test]
fn a_repair_with_no_router_named_is_sent_to_the_endpoint_stado_resolves_in_bramas_place() {
    let endpoint = std::env::var("PROBIERZ_TEST_MODEL_SERVICE_URL")
        .expect("declare the real model service URL");
    let model = std::env::var("PROBIERZ_TEST_MODEL_SERVICE_MODEL")
        .expect("declare the real model service model");
    let key = read(Path::new(
        &std::env::var("PROBIERZ_TEST_MODEL_SERVICE_KEY_FILE")
            .expect("declare the owner-only real model service bearer file"),
    ));
    let root = harness("resolved");
    let stado = Stado::isolated(&root);
    let store = root.join("credentials.json");
    stado.run(
        &[
            "config",
            "set",
            "credentials.store",
            &format!("file://{}", store.display()),
        ],
        "",
    );
    stado.run(
        &["credentials", "put", "--role", "probierz-journey-endpoint"],
        &json!({ "token": key.trim() }).to_string(),
    );
    for (field, value) in [
        ("capabilities.inference.endpoint.url", endpoint.as_str()),
        ("capabilities.inference.endpoint.model", model.as_str()),
        (
            "capabilities.inference.endpoint.key_role",
            "probierz-journey-endpoint",
        ),
    ] {
        stado.run(&["config", "set", field, value], "");
    }
    let output = repair(&root, |command| {
        stado.apply(command);
    });
    let result = answer(&output);
    let said = match &result["failure"]["detail"] {
        serde_json::Value::String(detail) => detail.clone(),
        _ => result.to_string(),
    };
    assert!(
        a_brief_was_written(&root),
        "the brief was written before the request: {result}"
    );
    assert!(
        !said.contains("STADO_MODEL_ROUTER_URL")
            && !said.contains("STADO_MODEL_ROUTER_TOKEN")
            && !said.contains("model router request to"),
        "the repair reached the resolved endpoint and was decided by its answer: {result}"
    );
    let record: serde_json::Value =
        serde_json::from_str(&stado.run(&["capabilities", "inference", "--json"], ""))
            .expect("stado answers JSON");
    assert_eq!(
        record["alternatives"][0]["served"]["probierz"]["provider"], "model-endpoint",
        "{record}"
    );
    std::fs::remove_dir_all(&root).expect("remove the harness");
}

/// A plain-HTTP model service on a host that only begins like this machine's
/// loopback address. Read by prefix it passed as loopback and was sent the
/// bearer in clear text; read from the parsed URL it is a foreign host.
const LOOK_ALIKE_LOOPBACK: &str = "http://127.model-service.invalid";

#[test]
fn a_plain_http_model_service_that_only_looks_like_loopback_is_refused() {
    let root = harness("look-alike");
    let output = repair(&root, |command| {
        command
            .env("STADO_MODEL_ROUTER_URL", LOOK_ALIKE_LOOPBACK)
            .env("STADO_MODEL_ROUTER_TOKEN", BEARER);
    });
    let result = answer(&output);
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(
        detail(&result),
        "STADO_MODEL_ROUTER_URL must use HTTPS or loopback HTTP",
        "{result}"
    );
    std::fs::remove_dir_all(&root).expect("remove the harness");
}
