//! The Stado fleet bridge.
//!
//! Submissions are source-bound, credentials remain vault references, and every
//! control-plane answer is retained before it is interpreted.  Remote workers
//! run this Rust binary from the submitted harness rather than a JavaScript
//! compatibility layer.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use clap::{Args, Subcommand};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::discovery;
use crate::failure::{print_json, Answer, Code, Failure};
use crate::manifest;

const STADO_BIN: &str = "stado";
const NODE_VERSION: &str = "v22.20.0";
const UPLOAD_ATTEMPTS: usize = 6;
const UPLOAD_BACKOFF: Duration = Duration::from_secs(5);
const WATCH_INTERVAL: Duration = Duration::from_secs(30);
const STATUS_TIMEOUT: Duration = Duration::from_secs(180);
const GUI_STATUS_TIMEOUT: Duration = Duration::from_secs(1800);
const STATUS_FAILURE_TOLERANCE: usize = 3;
const SETUP_STEP_TIMEOUT_MS: u64 = 30 * 60 * 1000;
const WATCH_BUDGET_ENV: &str = "PROBIERZ_WATCH_BUDGET_MS";
const STADO_RETRY_EXIT: i32 = 69;

const MODEL_ROUTER_REFERENCE: &str = "vault://wisent/probierz/model-router-token";
const MODEL_AGENT_REFERENCE: &str = "vault://wisent/probierz/model-agent-secret";
const SEO_KEY_REFERENCE: &str = "vault://wisent/probierz/seo-receipt-private-key";

#[derive(Debug, Subcommand)]
pub enum StadoCommand {
    /// Submit one target to the selected Stado host.
    Run(RunArgs),
    /// Read a job once and collect terminal evidence.
    Collect(CollectArgs),
    /// Continue watching an existing job without resubmitting it.
    Resume(ResumeArgs),
    /// Cancel a job and retain the complete cancellation attempt.
    Cancel(CancelArgs),
    /// Author and verify one journey on a Stado host.
    Author(AuthorArgs),
    /// Run the complete SEO evaluator remotely.
    Seo(SeoArgs),
    /// Internal source-bound receipt writer used only inside an author job.
    #[command(hide = true)]
    AuthorReceipt(AuthorReceiptArgs),
    /// Internal entry point copied to the dedicated iOS worker.
    #[command(hide = true)]
    BykAuthWorker,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    pub target: Option<String>,
    #[arg(long)]
    pub app: Option<String>,
    #[arg(long)]
    pub spec: Option<String>,
    #[arg(long)]
    pub record: bool,
    #[arg(long, default_value = "stado:gcp")]
    pub host: String,
    #[arg(long)]
    pub cargo_release: bool,
    #[arg(long)]
    pub app_repo: Option<PathBuf>,
    #[arg(long)]
    pub binary: Option<String>,
    #[arg(long)]
    pub cargo_manifest: Option<String>,
    #[arg(long, num_args = 0..=1)]
    pub app_binary_path: Option<Option<PathBuf>>,
    #[arg(long, num_args = 0..=1)]
    pub app_bundle_path: Option<Option<PathBuf>>,
    #[arg(long)]
    pub node_source: bool,
    #[arg(long, action = clap::ArgAction::Append)]
    pub env: Vec<String>,
    #[arg(long)]
    pub script: Option<String>,
    #[arg(long)]
    pub no_watch: bool,
}

#[derive(Debug, Args)]
pub struct CollectArgs {
    pub job_id: Option<String>,
    #[arg(long)]
    pub app: Option<String>,
    #[arg(long, default_value = "stado:mini")]
    pub host: String,
}

#[derive(Debug, Args)]
pub struct ResumeArgs {
    pub job_id: Option<String>,
    #[arg(long, default_value = "stado:any")]
    pub host: String,
}

#[derive(Debug, Args)]
pub struct CancelArgs {
    pub job_id: Option<String>,
    #[arg(long)]
    pub host: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Args)]
pub struct AuthorArgs {
    pub app_id: Option<String>,
    pub journey: Option<String>,
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long)]
    pub desc: Option<String>,
    #[arg(long)]
    pub area: Option<String>,
    #[arg(long, default_value = "stado:gcp")]
    pub host: String,
    #[arg(long, num_args = 0..=1)]
    pub app_path: Option<Option<PathBuf>>,
    #[arg(long, num_args = 0..=1)]
    pub app_binary_path: Option<Option<PathBuf>>,
    #[arg(long, num_args = 0..=1)]
    pub app_bundle_path: Option<Option<PathBuf>>,
    #[arg(long)]
    pub app_repo: Option<PathBuf>,
    #[arg(long)]
    pub cargo_release: bool,
    #[arg(long)]
    pub binary: Option<String>,
    #[arg(long)]
    pub cargo_manifest: Option<String>,
    #[arg(long)]
    pub no_watch: bool,
}

#[derive(Debug, Args)]
pub struct SeoArgs {
    pub app_id: Option<String>,
    #[arg(long)]
    pub base_url: Option<String>,
    #[arg(long, default_value = "release")]
    pub mode: String,
    #[arg(long)]
    pub policy: Option<String>,
    #[arg(long)]
    pub brief: Option<String>,
    #[arg(long)]
    pub production_evidence: Option<PathBuf>,
    #[arg(long)]
    pub primary_model: Option<String>,
    #[arg(long)]
    pub secondary_model: Option<String>,
    #[arg(long)]
    pub adjudicator_model: Option<String>,
    #[arg(long, default_value = "probierz")]
    pub agent_id: String,
    #[arg(long, default_value = "stado:mini")]
    pub host: String,
    #[arg(long)]
    pub no_watch: bool,
}

#[derive(Debug, Args)]
pub struct AuthorReceiptArgs {
    #[arg(long)]
    app: String,
    #[arg(long)]
    journey: String,
    #[arg(long)]
    area: String,
    #[arg(long)]
    target: String,
    #[arg(long)]
    receipt_id: String,
    #[arg(long)]
    result: PathBuf,
}

#[derive(Debug, Clone)]
enum Provision {
    InstalledTui {
        app_id: String,
        path: PathBuf,
    },
    NativeBinary {
        app_id: String,
        binary_path: PathBuf,
        binary_name: Option<String>,
        binary_sha256: Option<String>,
    },
    CargoRelease {
        app_id: String,
        binary: String,
        manifest_path: String,
    },
    AppBundle {
        app_id: String,
        bundle_path: PathBuf,
        bundle_name: Option<String>,
    },
    NodeSource {
        app_id: String,
        script: Option<String>,
    },
}

impl Provision {
    fn app_id(&self) -> &str {
        match self {
            Self::InstalledTui { app_id, .. }
            | Self::NativeBinary { app_id, .. }
            | Self::CargoRelease { app_id, .. }
            | Self::AppBundle { app_id, .. }
            | Self::NodeSource { app_id, .. } => app_id,
        }
    }
}

#[derive(Debug)]
struct ProcessOutput {
    command: String,
    args: Vec<String>,
    status: Option<i32>,
    signal: Option<i32>,
    stdout: String,
    stderr: String,
    error: Option<String>,
}

#[derive(Debug)]
struct Packed {
    file: PathBuf,
    hash: String,
}

#[derive(Debug)]
struct Identity {
    document: Value,
    file: PathBuf,
    hash: String,
}

#[derive(Debug)]
struct Submission {
    job_id: Option<String>,
    watch_budget_ms: u64,
    receipt_dir: PathBuf,
    failure: Option<Value>,
}

#[derive(Debug)]
struct Retained {
    results_dir: Option<PathBuf>,
    manifest: Option<Value>,
    author_receipt: Option<Value>,
    author_receipt_file: Option<PathBuf>,
    artifact_error: Option<Value>,
}

pub fn dispatch(harness: &Path, command: StadoCommand) -> Answer {
    match command {
        StadoCommand::Run(args) => dispatch_run(harness, args),
        StadoCommand::Collect(args) => dispatch_collect(harness, args),
        StadoCommand::Resume(args) => dispatch_resume(harness, args),
        StadoCommand::Cancel(args) => dispatch_cancel(harness, args),
        StadoCommand::Author(args) => dispatch_author(harness, args),
        StadoCommand::Seo(args) => dispatch_seo(harness, args),
        StadoCommand::AuthorReceipt(args) => write_author_receipt(harness, args),
        StadoCommand::BykAuthWorker => byk_auth_worker(),
    }
}

fn dispatch_run(harness: &Path, args: RunArgs) -> Answer {
    let target = args
        .target
        .as_deref()
        .ok_or_else(|| Failure::config("stado.run", "stado run needs a target (e.g. tui)"))?;
    let app_id = args
        .app
        .as_deref()
        .ok_or_else(|| Failure::config("stado.run", "stado run needs --app <appId>"))?;
    let environment = parse_environment(&args.env)?;
    let provision = select_run_provision(&args, app_id, target)?;
    if args.script.is_some() && !matches!(provision, Some(Provision::NodeSource { .. })) {
        return Err(Failure::config(
            "stado.run",
            "--script requires --node-source (custom app jobs run from app sources)",
        ));
    }
    let result = submit_remote_run(
        harness,
        target,
        app_id,
        args.spec.as_deref(),
        &args.host,
        provision,
        args.app_repo.as_deref(),
        !args.no_watch,
        if args.script.is_some() {
            "script"
        } else {
            "run"
        },
        args.record,
        &environment,
    )?;
    finish_remote(result)
}

fn dispatch_collect(harness: &Path, args: CollectArgs) -> Answer {
    let app_id = args
        .app
        .ok_or_else(|| Failure::config("stado.collect", "stado collect needs --app <appId>"))?;
    let result = collect_remote_run(harness, args.job_id.as_deref(), &app_id, &args.host)?;
    let must_finish = result
        .get("collected")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || matches!(
            result.get("state").and_then(Value::as_str),
            Some("failed" | "cancelled" | "evidence-unavailable")
        );
    print_json(&result)?;
    if must_finish {
        remote_result_answer(&result)
    } else {
        Ok(())
    }
}

fn dispatch_resume(harness: &Path, args: ResumeArgs) -> Answer {
    let result = resume_remote_run(harness, args.job_id.as_deref(), &args.host)?;
    finish_remote(result)
}

fn dispatch_cancel(harness: &Path, args: CancelArgs) -> Answer {
    let host = args
        .host
        .ok_or_else(|| Failure::config("stado.cancel", "stado cancel needs --host <host>"))?;
    let reason = args
        .reason
        .ok_or_else(|| Failure::config("stado.cancel", "stado cancel needs --reason <reason>"))?;
    let result = cancel_remote_run(harness, args.job_id.as_deref(), &host, &reason)?;
    print_json(&result)?;
    if result.get("cancellationSucceeded").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        remote_result_answer(&result)
    }
}

fn dispatch_author(harness: &Path, args: AuthorArgs) -> Answer {
    let app_id = args.app_id.as_deref().ok_or_else(|| {
        Failure::config(
            "stado.author",
            "stado author needs an app ID and a journey name",
        )
    })?;
    let journey = args.journey.as_deref().ok_or_else(|| {
        Failure::config(
            "stado.author",
            "stado author needs an app ID and a journey name",
        )
    })?;
    let target = args
        .target
        .as_deref()
        .ok_or_else(|| Failure::config("stado.author", "stado author needs --target <t>"))?;
    let desc = args.desc.as_deref().ok_or_else(|| {
        Failure::config("stado.author", "stado author needs --desc <journey goal>")
    })?;
    let provision = select_author_provision(&args, app_id, target)?;
    let result = submit_remote_author(
        harness,
        app_id,
        journey,
        target,
        desc,
        args.area.as_deref().unwrap_or(journey),
        &args.host,
        provision,
        args.app_repo.as_deref(),
        !args.no_watch,
    )?;
    finish_remote(result)
}

fn dispatch_seo(harness: &Path, args: SeoArgs) -> Answer {
    let app_id = args
        .app_id
        .clone()
        .ok_or_else(|| Failure::config("stado.seo", "stado seo needs an app ID"))?;
    let result = submit_remote_seo(harness, &app_id, args)?;
    finish_remote(result)
}

fn finish_remote(result: Value) -> Answer {
    print_json(&result)?;
    remote_result_answer(&result)
}

fn remote_result_answer(result: &Value) -> Answer {
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let queued = state == "queued"
        && result.get("submitted").and_then(Value::as_bool) == Some(true)
        && result.get("failure").map(Value::is_null).unwrap_or(true);
    if state == "completed" || queued {
        return Ok(());
    }
    let failure = result
        .get("cancellationFailure")
        .filter(|value| !value.is_null())
        .or_else(|| result.get("failure"));
    let point = failure
        .and_then(|value| value.get("failurePoint"))
        .and_then(Value::as_str)
        .unwrap_or("stado.remote");
    let message = failure
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("Remote run ended as \"{state}\"."));
    let retryable = failure
        .and_then(|value| value.get("retryable"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if retryable {
        Err(Failure::unavailable(point, message))
    } else {
        Err(Failure::config(point, message))
    }
}

fn parse_environment(values: &[String]) -> Result<Vec<(String, String)>, Failure> {
    let mut answer = Vec::with_capacity(values.len());
    for assignment in values {
        let Some((name, value)) = assignment.split_once('=') else {
            return Err(Failure::config(
                "stado.run",
                "--env needs NAME=VALUE with a valid environment variable name",
            ));
        };
        if !valid_environment_name(name) || value.contains('\0') {
            return Err(Failure::config(
                "stado.run",
                "--env needs NAME=VALUE with a valid environment variable name",
            ));
        }
        answer.push((name.to_string(), value.to_string()));
    }
    Ok(answer)
}

fn valid_environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn select_run_provision(
    args: &RunArgs,
    app_id: &str,
    target: &str,
) -> Result<Option<Provision>, Failure> {
    if let Some(None) = args.app_binary_path {
        return Err(Failure::config(
            "stado.run",
            "--app-binary-path needs a value",
        ));
    }
    if let Some(None) = args.app_bundle_path {
        return Err(Failure::config(
            "stado.run",
            "--app-bundle-path needs a value",
        ));
    }
    let mut candidates = Vec::new();
    if let Some(Some(path)) = &args.app_binary_path {
        candidates.push((
            "--app-binary-path",
            Provision::NativeBinary {
                app_id: app_id.to_string(),
                binary_path: path.clone(),
                binary_name: None,
                binary_sha256: None,
            },
        ));
    }
    if args.cargo_release {
        candidates.push((
            "--cargo-release",
            Provision::CargoRelease {
                app_id: app_id.to_string(),
                binary: args.binary.clone().unwrap_or_else(|| app_id.to_string()),
                manifest_path: args
                    .cargo_manifest
                    .clone()
                    .unwrap_or_else(|| "Cargo.toml".to_string()),
            },
        ));
    }
    if let Some(Some(path)) = &args.app_bundle_path {
        candidates.push((
            "--app-bundle-path",
            Provision::AppBundle {
                app_id: app_id.to_string(),
                bundle_path: path.clone(),
                bundle_name: None,
            },
        ));
    }
    if args.node_source {
        candidates.push((
            "--node-source",
            Provision::NodeSource {
                app_id: app_id.to_string(),
                script: args.script.clone(),
            },
        ));
    }
    validate_provision_candidates(
        &candidates,
        args.binary.is_some(),
        args.cargo_manifest.is_some(),
        args.cargo_release,
    )?;
    if args.app_binary_path.is_some() && args.app_repo.is_none() {
        return Err(Failure::config(
            "stado.run",
            "--app-binary-path requires --app-repo <path>",
        ));
    }
    if args.app_binary_path.is_some() && target != "tui" {
        return Err(Failure::config(
            "stado.run",
            "--app-binary-path is supported only for remote TUI runs",
        ));
    }
    Ok(candidates
        .into_iter()
        .next()
        .map(|(_, provision)| provision))
}

fn select_author_provision(
    args: &AuthorArgs,
    app_id: &str,
    target: &str,
) -> Result<Option<Provision>, Failure> {
    for (flag, value) in [
        ("--app-path", args.app_path.as_ref()),
        ("--app-binary-path", args.app_binary_path.as_ref()),
        ("--app-bundle-path", args.app_bundle_path.as_ref()),
    ] {
        if matches!(value, Some(None)) {
            return Err(Failure::config(
                "stado.author",
                format!("{flag} needs a value"),
            ));
        }
    }
    let mut candidates = Vec::new();
    if let Some(Some(path)) = &args.app_path {
        candidates.push((
            "--app-path",
            Provision::InstalledTui {
                app_id: app_id.to_string(),
                path: path.clone(),
            },
        ));
    }
    if let Some(Some(path)) = &args.app_binary_path {
        candidates.push((
            "--app-binary-path",
            Provision::NativeBinary {
                app_id: app_id.to_string(),
                binary_path: path.clone(),
                binary_name: None,
                binary_sha256: None,
            },
        ));
    }
    if args.cargo_release {
        candidates.push((
            "--cargo-release",
            Provision::CargoRelease {
                app_id: app_id.to_string(),
                binary: args.binary.clone().unwrap_or_else(|| app_id.to_string()),
                manifest_path: args
                    .cargo_manifest
                    .clone()
                    .unwrap_or_else(|| "Cargo.toml".to_string()),
            },
        ));
    }
    if let Some(Some(path)) = &args.app_bundle_path {
        candidates.push((
            "--app-bundle-path",
            Provision::AppBundle {
                app_id: app_id.to_string(),
                bundle_path: path.clone(),
                bundle_name: None,
            },
        ));
    }
    validate_provision_candidates(
        &candidates,
        args.binary.is_some(),
        args.cargo_manifest.is_some(),
        args.cargo_release,
    )?;
    if args.app_binary_path.is_some() && args.app_repo.is_none() {
        return Err(Failure::config(
            "stado.author",
            "--app-binary-path requires --app-repo <path>",
        ));
    }
    if args.app_binary_path.is_some() && target != "tui" {
        return Err(Failure::config(
            "stado.author",
            "--app-binary-path is supported only for remote TUI authoring",
        ));
    }
    if target == "tui"
        && !candidates.iter().any(|(_, provision)| {
            matches!(
                provision,
                Provision::InstalledTui { .. }
                    | Provision::NativeBinary { .. }
                    | Provision::CargoRelease { .. }
            )
        })
    {
        return Err(Failure::config(
            "stado.author",
            "stado author --target tui needs --app-path <installed-command>, --app-binary-path <file> --app-repo <path>, or --cargo-release --app-repo <path> [--binary <name>]",
        ));
    }
    Ok(candidates
        .into_iter()
        .next()
        .map(|(_, provision)| provision))
}

fn validate_provision_candidates(
    candidates: &[(&str, Provision)],
    has_binary: bool,
    has_manifest: bool,
    cargo_release: bool,
) -> Result<(), Failure> {
    if candidates.len() > 1 {
        let flags = candidates
            .iter()
            .map(|(flag, _)| *flag)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Failure::config(
            "stado.run",
            format!("remote application provisioning options are mutually exclusive: {flags}"),
        ));
    }
    if (has_binary || has_manifest) && !cargo_release {
        return Err(Failure::config(
            "stado.run",
            "--binary and --cargo-manifest require --cargo-release",
        ));
    }
    Ok(())
}

fn sh(
    command: &str,
    args: &[String],
    cwd: Option<&Path>,
    host: Option<&discovery::Host>,
    timeout: Option<Duration>,
) -> ProcessOutput {
    let mut process = Command::new(command);
    process
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(directory) = cwd {
        process.current_dir(directory);
    }
    if let Some(api_url) = host.and_then(|entry| entry.api_url) {
        process.env("STADO_API_URL", api_url);
    }
    let display_args = args.to_vec();
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ProcessOutput {
                command: command.to_string(),
                args: display_args,
                status: None,
                signal: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(error.to_string()),
            }
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stdout {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stderr {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    });
    let started = Instant::now();
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (Some(status), false),
            Ok(None)
                if timeout
                    .map(|limit| started.elapsed() >= limit)
                    .unwrap_or(false) =>
            {
                let _ = child.kill();
                break (child.wait().ok(), true);
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                let _ = child.kill();
                return ProcessOutput {
                    command: command.to_string(),
                    args: display_args,
                    status: None,
                    signal: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error: Some(error.to_string()),
                };
            }
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    ProcessOutput {
        command: command.to_string(),
        args: display_args,
        status: status.as_ref().and_then(|value| value.code()),
        signal: status.as_ref().and_then(|value| value.signal()),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        error: timed_out.then(|| "operation timed out".to_string()),
    }
}

fn process_text(output: &ProcessOutput) -> String {
    [
        output.error.as_deref(),
        Some(output.stderr.as_str()),
        Some(output.stdout.as_str()),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
    .trim()
    .to_string()
}

fn process_record(output: &ProcessOutput) -> Value {
    json!({
        "status": output.status,
        "signal": output.signal,
        "error": output.error.as_ref().map(|message| json!({ "code": Value::Null, "message": message })),
        "stdout": output.stdout,
        "stderr": output.stderr,
    })
}

fn remote_failure(point: &str, action: &str, output: &ProcessOutput) -> Failure {
    let diagnostic = json!({
        "failure_point": point,
        "command": output.command,
        "args": output.args,
        "exit_code": output.status,
        "stdout": output.stdout,
        "stderr": output.stderr,
        "error": output.error,
    });
    eprintln!("probierz-process-failure {diagnostic}");
    let exit = output
        .status
        .map(|code| code.to_string())
        .unwrap_or_else(|| "none".to_string());
    let text = process_text(output);
    Failure::unavailable(
        point,
        format!(
            "{action}: {STADO_BIN} exit {exit}{}",
            if text.is_empty() {
                String::new()
            } else {
                format!(" — {text}")
            }
        ),
    )
}

fn local_failure(point: &str, action: &str, output: &ProcessOutput) -> Failure {
    let exit = output
        .status
        .map(|code| code.to_string())
        .unwrap_or_else(|| "none".to_string());
    let text = process_text(output);
    Failure::config(
        point,
        format!(
            "{action}: tar exit {exit}{}",
            if text.is_empty() {
                String::new()
            } else {
                format!(" — {text}")
            }
        ),
    )
}

fn failure_summary(failure: &Failure, message: impl Into<String>) -> Value {
    json!({
        "failurePoint": failure.point,
        "errorCode": failure.code.as_str(),
        "service": "probierz",
        "retryable": failure.code.retryable(),
        "outage": failure.code.retryable(),
        "message": message.into(),
    })
}

fn host(name: &str, point: &str) -> Result<discovery::Host, Failure> {
    discovery::stado_host(name).ok_or_else(|| {
        Failure::config(
            point,
            format!("No such stado host: \"{name}\". Run `probierz hosts` for the list."),
        )
    })
}

fn require_gui_ready(target: &str, selected: &discovery::Host) -> Answer {
    if target != "desktop:cua" {
        return Ok(());
    }
    let registry_target = selected.target.ok_or_else(|| {
        Failure::config(
            "stado.preflight",
            format!(
                "The selected host \"{}\" cannot prove a usable macOS GUI session.",
                selected.host
            ),
        )
    })?;
    let started = Instant::now();
    let output = sh(
        STADO_BIN,
        &[
            "host".into(),
            "gui-automation".into(),
            "status".into(),
            registry_target.into(),
        ],
        None,
        Some(selected),
        Some(GUI_STATUS_TIMEOUT),
    );
    if output.error.as_deref() == Some("operation timed out") {
        return Err(Failure::config(
            "stado.preflight",
            format!("The GUI readiness audit for {registry_target} exceeded its deadline. Readiness is unknown; no GUI job was submitted. elapsed_ms={}", started.elapsed().as_millis()),
        ));
    }
    if output.status != Some(0) {
        return Err(remote_failure(
            "stado.preflight",
            &format!("Reading GUI readiness for {registry_target} failed"),
            &output,
        ));
    }
    let mut fields = BTreeMap::new();
    for line in output.stdout.lines() {
        let parts: Vec<&str> = line.trim().split('\t').collect();
        if parts.len() >= 3 {
            fields.insert(parts[1], parts[2..].join("\t"));
        }
    }
    let console = fields
        .get("console")
        .map(String::as_str)
        .unwrap_or("unknown");
    let accessibility = fields
        .get("accessibility")
        .map(String::as_str)
        .unwrap_or("unknown");
    let ready = !matches!(console, "" | "root" | "loginwindow" | "unknown")
        && fields.get("accessibility-user").map(String::as_str) == Some(console)
        && fields.get("automated-session-declared").map(String::as_str) == Some("yes")
        && fields.get("cua-driver-app").map(String::as_str) == Some("present")
        && accessibility == "granted";
    if !ready {
        return Err(Failure::config(
            "stado.preflight",
            format!("The selected host \"{}\" is not ready for desktop:cua: it needs an active macOS console session and a granted CuaDriver.", selected.host),
        ));
    }
    Ok(())
}

fn state_uri(kind: &str) -> String {
    format!("stado://probierz/{kind}")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn nonce(prefix: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(prefix.as_bytes());
    digest.update(now_millis().to_string().as_bytes());
    digest.update(std::process::id().to_string().as_bytes());
    digest.update(format!("{:?}", Instant::now()).as_bytes());
    hex::encode(digest.finalize())[..12].to_string()
}

fn work_path(name: &str) -> Result<PathBuf, Failure> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Failure::config("stado.pack", "HOME is required"))?;
    let directory = home.join(".stado").join("work").join("probierz");
    fs::create_dir_all(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    Ok(directory.join(name))
}

fn write_json(path: &Path, value: &Value, pretty: bool, newline: bool) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = if pretty {
        serde_json::to_vec_pretty(value)?
    } else {
        serde_json::to_vec(value)?
    };
    if newline {
        bytes.push(b'\n');
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, Failure> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn pack_source_tree(root: &Path, file: &Path, label: &str) -> Result<(), Failure> {
    let list = work_path(&format!(
        "source-list-{}-{}",
        std::process::id(),
        nonce(label)
    ))?;
    fs::write(&list, source_file_list(root)?)?;
    fs::set_permissions(&list, fs::Permissions::from_mode(0o600))?;
    let args = vec![
        "-czf".to_string(),
        file.display().to_string(),
        "--null".to_string(),
        "-T".to_string(),
        list.display().to_string(),
    ];
    let output = sh("tar", &args, Some(root), None, None);
    let _ = fs::remove_file(&list);
    if output.status != Some(0) {
        return Err(local_failure(
            "stado.pack",
            &format!("Packing {label} failed"),
            &output,
        ));
    }
    Ok(())
}

fn pack_repo(harness: &Path, app_ids: &[&str]) -> Result<Packed, Failure> {
    for app_id in app_ids {
        if !harness.join("apps").join(app_id).exists() {
            return Err(Failure::config(
                "stado.pack",
                format!("No app manifest for \"{app_id}\". Register it under apps/ before submitting a remote run."),
            ));
        }
    }
    let hash = nonce("probierz");
    let file = work_path(&format!("probierz-{hash}.tar.gz"))?;
    pack_source_tree(harness, &file, "the probierz checkout")?;
    Ok(Packed { file, hash })
}

fn pack_app_source(app_id: &str, repository: &Path) -> Result<Packed, Failure> {
    let hash = nonce(app_id);
    let file = work_path(&format!("{app_id}-{hash}.tar.gz"))?;
    pack_source_tree(repository, &file, &format!("the {app_id} source tree"))?;
    Ok(Packed { file, hash })
}

fn pack_app_bundle(app_id: &str, bundle: &Path) -> Result<(Packed, String), Failure> {
    let name = bundle
        .file_name()
        .and_then(|part| part.to_str())
        .ok_or_else(|| Failure::config("stado.pack", "application bundle has no file name"))?
        .to_string();
    let parent = bundle.parent().ok_or_else(|| {
        Failure::config("stado.pack", "application bundle has no parent directory")
    })?;
    let hash = nonce(&format!("{app_id}-app"));
    let file = work_path(&format!("{app_id}-app-{hash}.tar.gz"))?;
    let args = vec![
        "-czf".into(),
        file.display().to_string(),
        "-C".into(),
        parent.display().to_string(),
        name.clone(),
    ];
    let output = sh("tar", &args, None, None, None);
    if output.status != Some(0) {
        return Err(local_failure(
            "stado.pack",
            &format!("Packing the {app_id} application bundle failed"),
            &output,
        ));
    }
    Ok((Packed { file, hash }, name))
}

fn pack_source_identity(
    harness: &Path,
    app_id: &str,
    app_repo: Option<&Path>,
) -> Result<Identity, Failure> {
    let document = crate::authoring::app_source_identity(harness, app_id, app_repo)?;
    let compact = serde_json::to_vec(&document)?;
    let hash = hex::encode(Sha256::digest(&compact))[..12].to_string();
    let file = work_path(&format!("{app_id}-source-{hash}.json"))?;
    write_json(&file, &document, true, true)?;
    Ok(Identity {
        document,
        file,
        hash,
    })
}

fn upload(local_file: &Path, name: &str) -> Result<String, Failure> {
    upload_with(
        local_file,
        name,
        |destination, source| {
            sh(
                STADO_BIN,
                &[
                    "storage".into(),
                    "put".into(),
                    destination.into(),
                    source.display().to_string(),
                ],
                None,
                None,
                None,
            )
        },
        |duration| thread::sleep(duration),
    )
}

fn upload_with<F, S>(
    local_file: &Path,
    name: &str,
    mut call: F,
    mut sleep: S,
) -> Result<String, Failure>
where
    F: FnMut(&str, &Path) -> ProcessOutput,
    S: FnMut(Duration),
{
    let destination = format!("{}/{name}", state_uri("inputs"));
    let mut last = None;
    for attempt in 1..=UPLOAD_ATTEMPTS {
        let output = call(&destination, local_file);
        if output.status == Some(0) {
            return Ok(destination);
        }
        let retry = output.status == Some(STADO_RETRY_EXIT) && attempt < UPLOAD_ATTEMPTS;
        last = Some(output);
        if !retry {
            break;
        }
        sleep(UPLOAD_BACKOFF.saturating_mul(attempt as u32));
    }
    let mut output = last.expect("at least one upload attempt");
    output.stderr.push_str(&format!(
        " (source {}, {UPLOAD_ATTEMPTS} attempts)",
        local_file.display()
    ));
    Err(remote_failure(
        "stado.upload",
        &format!("Uploading {name} to the stado object store failed"),
        &output,
    ))
}

fn manifest_string<'a>(document: &'a serde_yaml::Value, path: &[&str]) -> Option<&'a str> {
    let mut current = document;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str()
}

fn remote_secret_env(harness: &Path, app_id: &str, names: &[&str]) -> Result<Value, Failure> {
    let application = manifest::load(harness, app_id)?;
    let configured = application
        .document
        .get("secretRefs")
        .and_then(serde_yaml::Value::as_mapping);
    let mut answer = Map::new();
    for name in names {
        let (reference, item, field) = match *name {
            "STADO_MODEL_ROUTER_TOKEN" => {
                (MODEL_ROUTER_REFERENCE, "probierz-model-router", "token")
            }
            "PROBIERZ_MODEL_AGENT_SECRET" => (
                MODEL_AGENT_REFERENCE,
                "probierz-agent-auth",
                "agent_auth_secret",
            ),
            "PROBIERZ_SEO_RECEIPT_PRIVATE_KEY" => (
                SEO_KEY_REFERENCE,
                "probierz-seo-receipt-signing",
                "private_key",
            ),
            _ => continue,
        };
        let found = configured
            .and_then(|mapping| mapping.get(serde_yaml::Value::from(*name)))
            .and_then(serde_yaml::Value::as_str);
        let Some(found) = found else {
            continue;
        };
        if found != reference {
            return Err(Failure::config(
                "stado.submit",
                format!("Remote runs require {reference} for {name}."),
            ));
        }
        answer.insert((*name).to_string(), json!({ "item": item, "field": field }));
    }
    Ok(Value::Object(answer))
}

fn setup_step_count(target: &str) -> Result<u64, Failure> {
    match target {
        "web" | "electron" | "mobile:ios" | "mobile:android" | "desktop:win" | "desktop:cua" => Ok(2),
        "desktop:mac" => Ok(3),
        "tui" => Ok(1),
        _ => Err(Failure::config("stado.submit", format!("unknown target: {target} (web|electron|mobile:ios|mobile:android|desktop:mac|desktop:cua|desktop:win|tui)"))),
    }
}

fn provisioning_budget(target: &str, provision: Option<&Provision>) -> Result<u64, Failure> {
    let setup = if target == "tui" {
        0
    } else {
        setup_step_count(target)?
    };
    let source_build = matches!(provision, Some(Provision::CargoRelease { .. })) as u64
        + (target == "desktop:cua"
            && matches!(provision, Some(Provision::AppBundle { app_id, .. }) if app_id == "stado"))
            as u64;
    Ok((1 + setup + source_build) * SETUP_STEP_TIMEOUT_MS)
}

fn selected_run_budget(
    harness: &Path,
    app_id: &str,
    target: &str,
    environment: &[(String, String)],
    provision: Option<&Provision>,
) -> Result<u64, Failure> {
    let application = manifest::load(harness, app_id)?;
    let surface = application
        .document
        .get("surfaces")
        .and_then(|surfaces| surfaces.get(target))
        .ok_or_else(|| {
            Failure::config(
                "stado.submit",
                format!("app {app_id} has no {target} surface"),
            )
        })?;
    let mut selected = BTreeMap::new();
    if let Some(conditions) = surface
        .get("conditions")
        .and_then(serde_yaml::Value::as_mapping)
    {
        for (name, value) in conditions {
            if let (Some(name), Some(value)) = (name.as_str(), yaml_scalar(value)) {
                selected.insert(name.to_string(), value);
            }
        }
    }
    for (name, value) in environment {
        selected.insert(name.clone(), value.clone());
    }
    if let Some(bindings) = surface.get("env").and_then(serde_yaml::Value::as_mapping) {
        for (target_name, source_name) in bindings {
            let (Some(target_name), Some(source_name)) =
                (target_name.as_str(), source_name.as_str())
            else {
                continue;
            };
            if let Some(value) = selected
                .get(source_name)
                .cloned()
                .or_else(|| std::env::var(source_name).ok())
            {
                selected.insert(source_name.to_string(), value.clone());
                selected.insert(target_name.to_string(), value);
            }
        }
    }
    let journeys = manifest::surface_journeys(surface, &selected);
    let mut budget = 0_u64;
    for journey in journeys {
        budget = budget.saturating_add(
            application
                .document
                .get("journeys")
                .and_then(|all| all.get(&journey))
                .and_then(|value| value.get("timeoutMs"))
                .and_then(serde_yaml::Value::as_u64)
                .unwrap_or(0),
        );
    }
    Ok(budget.saturating_add(provisioning_budget(target, provision)?))
}

fn conservative_watch_budget(harness: &Path, app_id: &str) -> Result<u64, Failure> {
    let application = manifest::load(harness, app_id)?;
    let journey_budget = application
        .document
        .get("journeys")
        .and_then(serde_yaml::Value::as_mapping)
        .map(|journeys| {
            journeys
                .values()
                .filter_map(|journey| journey.get("timeoutMs").and_then(serde_yaml::Value::as_u64))
                .sum()
        })
        .unwrap_or(0_u64);
    let surfaces = application
        .document
        .get("surfaces")
        .and_then(serde_yaml::Value::as_mapping);
    let mut provision = 0_u64;
    if let Some(surfaces) = surfaces {
        for target in surfaces.keys().filter_map(serde_yaml::Value::as_str) {
            provision = provision.max(provisioning_budget(
                target,
                Some(&Provision::CargoRelease {
                    app_id: app_id.to_string(),
                    binary: app_id.to_string(),
                    manifest_path: "Cargo.toml".to_string(),
                }),
            )?);
        }
    }
    Ok(journey_budget.saturating_add(provision))
}

fn yaml_scalar(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(text) => Some(text.clone()),
        serde_yaml::Value::Number(number) => Some(number.to_string()),
        serde_yaml::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn run_script(
    target: &str,
    app_id: &str,
    spec: Option<&str>,
    provision: Option<&Provision>,
    hash: &str,
    platform: Option<&str>,
    mode: &str,
    author: Option<(&str, &str, &str, &str)>,
    model_router_url: Option<&str>,
    record: bool,
    environment: &[(String, String)],
) -> Result<String, Failure> {
    let mut lines = vec![
        "set -euo pipefail".to_string(),
        "JOB_ROOT=\"$PWD\"".to_string(),
        "mkdir -p output work".to_string(),
        "export TMPDIR=\"$JOB_ROOT/work\"".to_string(),
        "export CARGO_HOME=\"${CARGO_HOME:-$HOME/.cargo}\"".to_string(),
        "export RUSTUP_HOME=\"${RUSTUP_HOME:-$HOME/.rustup}\"".to_string(),
        "export PATH=\"$CARGO_HOME/bin:$PATH\"".to_string(),
    ];
    if platform == Some("darwin") {
        lines.extend([
            "export PATH=$HOME/.stado/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH".into(),
            format!(
                "command -v node >/dev/null 2>&1 || {{ curl -fsSL https://nodejs.org/dist/{NODE_VERSION}/node-{NODE_VERSION}-darwin-arm64.tar.gz -o \"$TMPDIR/node.tar.gz\" && tar -xzf \"$TMPDIR/node.tar.gz\" -C \"$TMPDIR\" && export PATH=\"$TMPDIR/node-{NODE_VERSION}-darwin-arm64/bin:$PATH\"; }}",
            ),
        ]);
    } else if platform == Some("linux") {
        lines.extend([
            format!("curl -fsSL https://nodejs.org/dist/{NODE_VERSION}/node-{NODE_VERSION}-linux-x64.tar.xz -o \"$TMPDIR/node.tar.xz\""),
            "tar -xJf \"$TMPDIR/node.tar.xz\" -C \"$TMPDIR\"".into(),
            format!("export PATH=\"$TMPDIR/node-{NODE_VERSION}-linux-x64/bin:$PATH\""),
        ]);
    } else {
        lines.extend([
            "readonly PROBIERZ_WORKER_OS=\"$(uname -s)\"".into(),
            "readonly PROBIERZ_WORKER_ARCH=\"$(uname -m)\"".into(),
            "case \"$PROBIERZ_WORKER_OS:$PROBIERZ_WORKER_ARCH\" in".into(),
            "  Darwin:arm64) PROBIERZ_NODE_PLATFORM=darwin-arm64; PROBIERZ_NODE_EXTENSION=tar.gz ;;".into(),
            "  Darwin:x86_64) PROBIERZ_NODE_PLATFORM=darwin-x64; PROBIERZ_NODE_EXTENSION=tar.gz ;;".into(),
            "  Linux:aarch64|Linux:arm64) PROBIERZ_NODE_PLATFORM=linux-arm64; PROBIERZ_NODE_EXTENSION=tar.xz ;;".into(),
            "  Linux:x86_64|Linux:amd64) PROBIERZ_NODE_PLATFORM=linux-x64; PROBIERZ_NODE_EXTENSION=tar.xz ;;".into(),
            "  *) printf 'Unsupported Stado worker OS/architecture: %s/%s (supported: Darwin or Linux on arm64 or x64)\\n' \"$PROBIERZ_WORKER_OS\" \"$PROBIERZ_WORKER_ARCH\" >&2; exit 1 ;;".into(),
            "esac".into(),
            "if [ \"$PROBIERZ_WORKER_OS\" = Darwin ]; then export PATH=$HOME/.stado/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH; fi".into(),
            "if ! command -v node >/dev/null 2>&1; then".into(),
            format!("  PROBIERZ_NODE_ARCHIVE=\"node-{NODE_VERSION}-$PROBIERZ_NODE_PLATFORM.$PROBIERZ_NODE_EXTENSION\""),
            format!("  curl -fsSL \"https://nodejs.org/dist/{NODE_VERSION}/$PROBIERZ_NODE_ARCHIVE\" -o \"$TMPDIR/$PROBIERZ_NODE_ARCHIVE\""),
            "  case \"$PROBIERZ_NODE_EXTENSION\" in".into(),
            "    tar.gz) tar -xzf \"$TMPDIR/$PROBIERZ_NODE_ARCHIVE\" -C \"$TMPDIR\" ;;".into(),
            "    tar.xz) tar -xJf \"$TMPDIR/$PROBIERZ_NODE_ARCHIVE\" -C \"$TMPDIR\" ;;".into(),
            "  esac".into(),
            format!("  export PATH=\"$TMPDIR/node-{NODE_VERSION}-$PROBIERZ_NODE_PLATFORM/bin:$PATH\""),
            "fi".into(),
        ]);
    }
    lines.extend([
        "mkdir -p \"$JOB_ROOT/work/probierz\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/probierz.tar.gz\" -C \"$JOB_ROOT/work/probierz\"".into(),
        "command -v cargo >/dev/null 2>&1 || { curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal; }".into(),
        "cargo build --locked --release --manifest-path \"$JOB_ROOT/work/probierz/probierz-rs/Cargo.toml\" --bin probierz".into(),
        "PROBIERZ=\"$JOB_ROOT/work/probierz/probierz-rs/target/release/probierz\"".into(),
        "HARNESS=\"$JOB_ROOT/work/probierz\"".into(),
    ]);
    if let Some(provision) = provision {
        match provision {
            Provision::InstalledTui { path, .. } => {
                lines.push(format!(
                    "export TUI_CMD={}",
                    shell_quote(&path.display().to_string())
                ));
            }
            Provision::NativeBinary { app_id, .. } => {
                lines.extend([
                    format!("mkdir -p \"$JOB_ROOT/work/{app_id}\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/{app_id}.tar.gz\" -C \"$JOB_ROOT/work/{app_id}\""),
                    format!("export PROBIERZ_APP_SOURCE=\"$JOB_ROOT/work/{app_id}\""),
                    format!("cp \"$JOB_ROOT/inputs/{app_id}.binary\" \"$JOB_ROOT/work/{app_id}-binary\""),
                    format!("chmod 0755 \"$JOB_ROOT/work/{app_id}-binary\""),
                    format!("export TUI_CMD=\"$JOB_ROOT/work/{app_id}-binary\""),
                    "export PROBIERZ_BUILD_PATH=\"$TUI_CMD\"".into(),
                ]);
            }
            Provision::CargoRelease {
                app_id,
                binary,
                manifest_path,
            } => {
                let manifest_dir = Path::new(manifest_path)
                    .parent()
                    .filter(|path| !path.as_os_str().is_empty())
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| ".".to_string());
                let target_prefix = if manifest_dir == "." {
                    String::new()
                } else {
                    format!("{manifest_dir}/")
                };
                lines.extend([
                    format!("mkdir -p \"$JOB_ROOT/work/{app_id}\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/{app_id}.tar.gz\" -C \"$JOB_ROOT/work/{app_id}\""),
                    format!("export PROBIERZ_APP_SOURCE=\"$JOB_ROOT/work/{app_id}\""),
                    format!("readonly PROBIERZ_CARGO_TARGET_DIR=\"$PROBIERZ_APP_SOURCE/{target_prefix}target\""),
                    "export CARGO_TARGET_DIR=\"$PROBIERZ_CARGO_TARGET_DIR\"".into(),
                    "trap 'rm -rf -- \"$PROBIERZ_CARGO_TARGET_DIR\"' EXIT".into(),
                    "command -v cargo >/dev/null 2>&1 || { curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal; }".into(),
                    format!("(cd \"$PROBIERZ_APP_SOURCE/{manifest_dir}\" && cargo build --locked --release --bins)"),
                    format!("export TUI_CMD=\"$JOB_ROOT/work/{app_id}/{target_prefix}target/release/{binary}\""),
                ]);
            }
            Provision::AppBundle {
                app_id,
                bundle_name,
                ..
            } => {
                let name = bundle_name.as_deref().ok_or_else(|| {
                    Failure::config("stado.pack", "application bundle was not staged")
                })?;
                lines.extend([
                    format!("mkdir -p \"$JOB_ROOT/work/{app_id}\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/{app_id}-app.tar.gz\" -C \"$JOB_ROOT/work/{app_id}\""),
                    format!("export MAC_APP_PATH=\"$JOB_ROOT/work/{app_id}/{name}\""),
                    format!("mkdir -p \"$JOB_ROOT/work/{app_id}-src\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/{app_id}.tar.gz\" -C \"$JOB_ROOT/work/{app_id}-src\""),
                    format!("export PROBIERZ_APP_SOURCE=\"$JOB_ROOT/work/{app_id}-src\""),
                ]);
                if target == "desktop:cua" {
                    lines.extend([
                        "CUA_EXECUTABLE=$(/usr/libexec/PlistBuddy -c \"Print :CFBundleExecutable\" \"$MAC_APP_PATH/Contents/Info.plist\")".into(),
                        "export CUA_APP_EXECUTABLE=\"$MAC_APP_PATH/Contents/MacOS/$CUA_EXECUTABLE\"".into(),
                    ]);
                }
            }
            Provision::NodeSource { app_id, .. } => {
                lines.extend([
                    format!("mkdir -p \"$JOB_ROOT/work/{app_id}\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/{app_id}.tar.gz\" -C \"$JOB_ROOT/work/{app_id}\""),
                    format!("export PROBIERZ_APP_SOURCE=\"$JOB_ROOT/work/{app_id}\""),
                ]);
            }
        }
    }
    if mode == "author" && matches!(provision, None | Some(Provision::InstalledTui { .. })) {
        lines.extend([
            format!("mkdir -p \"$JOB_ROOT/work/{app_id}\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/{app_id}.tar.gz\" -C \"$JOB_ROOT/work/{app_id}\""),
            format!("export PROBIERZ_APP_SOURCE=\"$JOB_ROOT/work/{app_id}\""),
        ]);
    }
    for (name, value) in environment {
        lines.push(format!("export {name}={}", shell_quote(value)));
    }
    lines.extend([
        "cd \"$HARNESS\"".into(),
        "export PROBIERZ_SOURCE_IDENTITY=\"$JOB_ROOT/inputs/source-identity.json\"".into(),
    ]);
    if mode == "author" || (mode != "run" && provision.is_some()) {
        let source = match provision {
            Some(Provision::AppBundle { app_id, .. }) => format!("$JOB_ROOT/work/{app_id}-src"),
            Some(value) => format!("$JOB_ROOT/work/{}", value.app_id()),
            None => format!("$JOB_ROOT/work/{app_id}"),
        };
        lines.push(format!(
            "perl -pi -e \"s|^  - root: .*|  - root: {source}|\" apps/{app_id}/probierz.yaml"
        ));
    }
    if matches!(
        target,
        "mobile:ios" | "mobile:android" | "desktop:mac" | "desktop:win"
    ) {
        let appium = if target == "desktop:mac" {
            "appium-2-mac2-2.2.2"
        } else {
            "appium-2"
        };
        lines.push(format!(
            "export APPIUM_HOME=\"$HOME/.cache/probierz/{appium}\""
        ));
    }
    lines.push("npm ci --no-audit --no-fund --loglevel=error".into());
    if target != "tui" {
        lines.push(format!(
            "\"$PROBIERZ\" --harness \"$HARNESS\" setup {target}"
        ));
    }
    if mode == "script" {
        let script = match provision {
            Some(Provision::NodeSource {
                script: Some(script),
                ..
            }) => script,
            _ => {
                return Err(Failure::config(
                    "stado.submit",
                    "script mode needs a staged node-source script",
                ))
            }
        };
        lines.extend([
            "mkdir -p test-results".into(),
            "set +e".into(),
            format!("bash apps/{app_id}/{script}"),
            "PROBIERZ_RUN_RC=$?".into(),
            "set -e".into(),
            format!("tar -czf \"$JOB_ROOT/output/probierz-run-{hash}.tar.gz\" test-results"),
            "exit $PROBIERZ_RUN_RC".into(),
        ]);
        return Ok(lines.join("\n"));
    }
    if mode == "author" {
        let (journey, area, description, receipt_id) = author.ok_or_else(|| {
            Failure::config(
                "stado.submit",
                "remote authoring needs its journey contract",
            )
        })?;
        let router = model_router_url
            .ok_or_else(|| Failure::config("stado.submit", "STADO_MODEL_ROUTER_URL is required"))?;
        lines.extend([
            format!("export STADO_MODEL_ROUTER_URL={}", shell_quote(router)),
            ": \"${STADO_MODEL_ROUTER_TOKEN:?STADO_MODEL_ROUTER_TOKEN was not materialized by Stado}\"".into(),
            "export PROBIERZ_MODEL_AGENT_ID=probierz".into(),
            ": \"${PROBIERZ_MODEL_AGENT_SECRET:?PROBIERZ_MODEL_AGENT_SECRET was not materialized by Stado}\"".into(),
            format!("export PROBIERZ_AUTHOR_RECEIPT_ID={}", shell_quote(receipt_id)),
        ]);
        if matches!(
            target,
            "mobile:ios" | "mobile:android" | "desktop:mac" | "desktop:win"
        ) {
            lines.extend([
                "pkill -f '[a]ppium.*--port 4723' >/dev/null 2>&1 || true".into(),
                "npx appium --relaxed-security --port 4723 > /tmp/appium.log 2>&1 &".into(),
                "APPIUM_PID=$!".into(),
                "trap 'kill \"$APPIUM_PID\" >/dev/null 2>&1 || true' EXIT".into(),
                "export PROBIERZ_EXTERNAL_APPIUM=1".into(),
                "for i in $(seq 1 30); do nc -z 127.0.0.1 4723 && break; sleep 2; done".into(),
                "nc -z 127.0.0.1 4723".into(),
            ]);
        }
        let app_path = if target == "web" {
            String::new()
        } else if target == "tui" {
            " --app-path \"$TUI_CMD\"".into()
        } else {
            " --app-path \"$MAC_APP_PATH\"".into()
        };
        lines.extend([
            "set +e".into(),
            format!(
                "\"$PROBIERZ\" --harness \"$HARNESS\" author-spec {} {} --area {} --target {} --desc {}{app_path} > \"$JOB_ROOT/work/author-result.json\"",
                shell_quote(app_id), shell_quote(journey), shell_quote(area), shell_quote(target), shell_quote(description),
            ),
            "PROBIERZ_RC=$?".into(),
            "set -e".into(),
            "if [ \"$PROBIERZ_RC\" -eq 0 ]; then".into(),
            format!(
                "  \"$PROBIERZ\" --harness \"$HARNESS\" stado author-receipt --app {} --journey {} --area {} --target {} --receipt-id {} --result \"$JOB_ROOT/work/author-result.json\"",
                shell_quote(app_id), shell_quote(journey), shell_quote(area), shell_quote(target), shell_quote(receipt_id),
            ),
            "fi".into(),
            "mkdir -p test-results".into(),
            format!("tar -czf \"$JOB_ROOT/output/probierz-author-{hash}.tar.gz\" test-results"),
            "exit $PROBIERZ_RC".into(),
        ]);
        return Ok(lines.join("\n"));
    }
    let has_source = matches!(
        provision,
        Some(
            Provision::AppBundle { .. }
                | Provision::CargoRelease { .. }
                | Provision::NativeBinary { .. }
                | Provision::NodeSource { .. }
        )
    );
    let mut conditions = vec!["PROBIERZ_RUN_KIND=pull-request".to_string()];
    if target == "tui" && !matches!(provision, Some(Provision::NodeSource { .. })) {
        conditions.push("TUI_CMD=\"$TUI_CMD\"".into());
    }
    if has_source {
        conditions.push("PROBIERZ_APP_SOURCE=\"$PROBIERZ_APP_SOURCE\"".into());
    }
    if target == "desktop:cua" {
        conditions.push("CUA_APP_EXECUTABLE=\"$CUA_APP_EXECUTABLE\"".into());
    }
    conditions.extend(
        environment
            .iter()
            .map(|(name, value)| shell_quote(&format!("{name}={value}"))),
    );
    let source_flag = if has_source {
        " --app-repo \"$PROBIERZ_APP_SOURCE\""
    } else {
        ""
    };
    let spec_flag = spec
        .map(|value| format!(" --spec {value}"))
        .unwrap_or_default();
    let record_flag = if record { " --record" } else { "" };
    lines.extend([
        "set +e".into(),
        format!("\"$PROBIERZ\" --harness \"$HARNESS\" run {target} --app {app_id}{source_flag}{spec_flag}{record_flag} {}", conditions.join(" ")),
        "PROBIERZ_RUN_RC=$?".into(),
        "set -e".into(),
        format!("tar -czf \"$JOB_ROOT/output/probierz-run-{hash}.tar.gz\" test-results"),
        "exit $PROBIERZ_RUN_RC".into(),
    ]);
    Ok(lines.join("\n"))
}

fn write_author_receipt(harness: &Path, args: AuthorReceiptArgs) -> Answer {
    let receipt_id = safe_author_name(&args.receipt_id, "authoring receipt")?;
    let application = manifest::load(harness, &args.app)?;
    let test_directory = application
        .document
        .get("surfaces")
        .and_then(|value| value.get(&args.target))
        .and_then(|value| value.get("testDirectory"))
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("tests");
    let result = read_json(&args.result).ok_or_else(|| {
        Failure::config(
            "stado.author-receipt",
            "Remote authoring did not return a readable result.",
        )
    })?;
    if result.get("ok").and_then(Value::as_bool) != Some(true)
        || result.get("journey").and_then(Value::as_str) != Some(args.journey.as_str())
        || result.get("target").and_then(Value::as_str) != Some(args.target.as_str())
    {
        return Err(Failure::config(
            "stado.author-receipt",
            "Remote authoring did not return an accepted spec.",
        ));
    }
    let run_id = result.get("runId").and_then(Value::as_str).ok_or_else(|| {
        Failure::config(
            "stado.author-receipt",
            "Remote authoring did not return its verification run.",
        )
    })?;
    let registration_dir = registration_directory(&args.target).ok_or_else(|| {
        Failure::config(
            "stado.author-receipt",
            format!(
                "Remote authoring does not support target \"{}\".",
                args.target
            ),
        )
    })?;
    let registration_relative = format!(
        "{registration_dir}/{}-{}{}",
        args.app,
        args.journey,
        registration_extension(&args.target),
    );
    let returned_spec = result
        .get("spec")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| {
            Failure::config(
                "stado.author-receipt",
                "Remote authoring did not return its accepted spec path.",
            )
        })?;
    if returned_spec != harness.join(&registration_relative) || !returned_spec.is_file() {
        return Err(Failure::config(
            "stado.author-receipt",
            "Remote authoring returned an accepted spec outside its registration path.",
        ));
    }
    let run_manifest = harness
        .join("test-results")
        .join(run_id)
        .join("run-manifest.json");
    let retained_manifest = read_json(&run_manifest).ok_or_else(|| {
        Failure::config(
            "stado.author-receipt",
            "Remote authoring completed without its verification manifest.",
        )
    })?;
    if retained_manifest.get("runId").and_then(Value::as_str) != Some(run_id)
        || retained_manifest.get("appId").and_then(Value::as_str) != Some(args.app.as_str())
        || retained_manifest.get("target").and_then(Value::as_str) != Some(args.target.as_str())
        || retained_manifest.get("status").and_then(Value::as_str) != Some("passed")
    {
        return Err(Failure::config(
            "stado.author-receipt",
            "Remote authoring verification did not retain a passing source-bound manifest.",
        ));
    }
    let identity_file = std::env::var_os("PROBIERZ_SOURCE_IDENTITY")
        .map(PathBuf::from)
        .ok_or_else(|| {
            Failure::config(
                "stado.author-receipt",
                "Remote authoring has no submitting source identity.",
            )
        })?;
    let identity = read_json(&identity_file).ok_or_else(|| {
        Failure::config(
            "stado.author-receipt",
            "Remote authoring has no readable submitting source identity.",
        )
    })?;
    if identity.get("appId").and_then(Value::as_str) != Some(args.app.as_str()) {
        return Err(Failure::config(
            "stado.author-receipt",
            "Remote authoring source identity does not name the submitted app.",
        ));
    }
    let source_sha = identity
        .pointer("/app/sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Failure::config(
                "stado.author-receipt",
                "Remote authoring source identity has no app digest.",
            )
        })?;
    let harness_sha = identity
        .pointer("/harness/sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Failure::config(
                "stado.author-receipt",
                "Remote authoring source identity has no harness digest.",
            )
        })?;
    if retained_manifest
        .pointer("/source/sha256")
        .and_then(Value::as_str)
        != Some(source_sha)
        || retained_manifest
            .pointer("/harness/sha256")
            .and_then(Value::as_str)
            != Some(harness_sha)
        || retained_manifest
            .get("sourceIdentityOrigin")
            .and_then(Value::as_str)
            != Some("submitter")
    {
        return Err(Failure::config(
            "stado.author-receipt",
            "Remote authoring verification does not match the submitting source identity.",
        ));
    }
    let accepted = fs::read(&returned_spec)?;
    let digest = hex::encode(Sha256::digest(&accepted));
    let receipt_root = harness
        .join("test-results")
        .join(".authoring")
        .join(&args.app)
        .join(receipt_id);
    fs::create_dir_all(&receipt_root)?;
    let artifact_file =
        receipt_root.join(format!("accepted-spec.{}", product_extension(&args.target)));
    fs::write(&artifact_file, &accepted)?;
    let artifact_relative = artifact_file
        .strip_prefix(harness)
        .map_err(|_| {
            Failure::config(
                "stado.author-receipt",
                "Accepted spec is outside retained Probierz artifacts.",
            )
        })?
        .to_string_lossy()
        .to_string();
    let product_relative = format!(
        "{}/{}/{}.probierz.spec.{}",
        test_directory,
        args.area,
        args.journey,
        product_extension(&args.target),
    );
    let receipt = json!({
        "schemaVersion": 1,
        "appId": args.app,
        "journey": args.journey,
        "area": args.area,
        "target": args.target,
        "runId": run_id,
        "sourceSha256": source_sha,
        "harnessSha256": harness_sha,
        "spec": {
            "relativePath": product_relative,
            "artifact": artifact_relative,
            "bytes": accepted.len(),
            "sha256": digest,
        },
        "registration": { "relativePath": registration_relative },
        "mappingPaths": [],
    });
    let receipt_file = receipt_root.join("accepted.json");
    write_json(&receipt_file, &receipt, true, true)?;
    fs::set_permissions(&receipt_file, fs::Permissions::from_mode(0o600))?;
    print_json(&json!({ "ok": true, "receipt": receipt_file }))
}

fn seo_script(
    app_id: &str,
    base_url: &str,
    mode: &str,
    policy: &str,
    brief: &str,
    primary: &str,
    secondary: &str,
    adjudicator: &str,
    agent_id: &str,
    router_url: &str,
    production_evidence: bool,
    signature_required: bool,
    hash: &str,
) -> String {
    let mut lines = vec![
        "set -euo pipefail".to_string(),
        "JOB_ROOT=\"$PWD\"".into(),
        "mkdir -p output work".into(),
        "export CARGO_HOME=\"${CARGO_HOME:-$HOME/.cargo}\"".into(),
        "export RUSTUP_HOME=\"${RUSTUP_HOME:-$HOME/.rustup}\"".into(),
        "export PATH=\"$HOME/.stado/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$CARGO_HOME/bin:$PATH\"".into(),
        "mkdir -p \"$JOB_ROOT/work/probierz\" && tar --no-same-owner -xzf \"$JOB_ROOT/inputs/probierz.tar.gz\" -C \"$JOB_ROOT/work/probierz\"".into(),
        "command -v cargo >/dev/null 2>&1 || { curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal; }".into(),
        "cargo build --locked --release --manifest-path \"$JOB_ROOT/work/probierz/probierz-rs/Cargo.toml\" --bin probierz".into(),
        "PROBIERZ=\"$JOB_ROOT/work/probierz/probierz-rs/target/release/probierz\"".into(),
        "HARNESS=\"$JOB_ROOT/work/probierz\"".into(),
        format!("export STADO_MODEL_ROUTER_URL={}", shell_quote(router_url)),
        format!("export PROBIERZ_MODEL_AGENT_ID={}", shell_quote(agent_id)),
        ": \"${STADO_MODEL_ROUTER_TOKEN:?STADO_MODEL_ROUTER_TOKEN was not materialized by Stado}\"".into(),
        ": \"${PROBIERZ_MODEL_AGENT_SECRET:?PROBIERZ_MODEL_AGENT_SECRET was not materialized by Stado}\"".into(),
    ];
    if signature_required {
        lines.push(": \"${PROBIERZ_SEO_RECEIPT_PRIVATE_KEY:?PROBIERZ_SEO_RECEIPT_PRIVATE_KEY was not materialized by Stado}\"".into());
    }
    let mut command = format!(
        "\"$PROBIERZ\" --harness \"$HARNESS\" seo-evaluate --app {} --base-url {} --mode {} --policy {} --brief {} --primary-model {} --secondary-model {} --adjudicator-model {} --agent-id {}",
        shell_quote(app_id), shell_quote(base_url), shell_quote(mode), shell_quote(policy), shell_quote(brief),
        shell_quote(primary), shell_quote(secondary), shell_quote(adjudicator), shell_quote(agent_id),
    );
    if production_evidence {
        command.push_str(" --production-evidence \"$JOB_ROOT/inputs/production-evidence.json\"");
    }
    lines.extend([
        "set +e".into(),
        command,
        "PROBIERZ_SEO_RC=$?".into(),
        "set -e".into(),
        format!("tar -czf \"$JOB_ROOT/output/probierz-seo-{hash}.tar.gz\" test-results"),
        "exit $PROBIERZ_SEO_RC".into(),
    ]);
    lines.join("\n")
}

fn provision_inputs(
    app_id: &str,
    provision: &mut Option<Provision>,
    app_repo: Option<&Path>,
    source_required: bool,
) -> Result<Map<String, Value>, Failure> {
    let mut inputs = Map::new();
    match provision {
        Some(Provision::InstalledTui { path, .. }) => {
            if !path.is_absolute() {
                return Err(Failure::config(
                    "stado.pack",
                    "Remote installed-TUI authoring needs --app-path <absolute-path>.",
                ));
            }
            if source_required {
                let repository = app_repo.ok_or_else(|| Failure::config("stado.pack", "Remote installed-TUI authoring needs --app-repo <path> or a manifest repository root."))?;
                insert_source_input(&mut inputs, app_id, repository)?;
            }
        }
        Some(Provision::NativeBinary {
            binary_path,
            binary_name,
            binary_sha256,
            ..
        }) => {
            let repository = app_repo.ok_or_else(|| {
                Failure::config(
                    "stado.pack",
                    "Remote native-binary provisioning needs --app-repo <path>.",
                )
            })?;
            if !binary_path.is_file() {
                return Err(Failure::config("stado.pack", "The --app-binary-path you gave is not a file. Supply the signed native executable."));
            }
            let staged = work_path(&format!(
                "{app_id}-binary-{}-{}",
                now_millis(),
                std::process::id()
            ))?;
            fs::copy(&binary_path, &staged)?;
            let digest = hash_file(&staged)?;
            *binary_name = binary_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string);
            *binary_sha256 = Some(digest.clone());
            inputs.insert(
                "binary".into(),
                json!({
                    "stado_uri": upload(&staged, &format!("{app_id}-binary-{digest}"))?,
                    "relative_path": format!("inputs/{app_id}.binary"),
                }),
            );
            insert_source_input(&mut inputs, app_id, repository)?;
        }
        Some(Provision::CargoRelease { manifest_path, .. }) => {
            let repository = app_repo.ok_or_else(|| {
                Failure::config(
                    "stado.pack",
                    "Remote cargo-release provisioning needs --app-repo <path>.",
                )
            })?;
            if !safe_relative_path(manifest_path) {
                return Err(Failure::config(
                    "stado.pack",
                    "--cargo-manifest must be a safe path relative to --app-repo.",
                ));
            }
            insert_source_input(&mut inputs, app_id, repository)?;
        }
        Some(Provision::NodeSource { .. }) => {
            let repository = app_repo.ok_or_else(|| {
                Failure::config(
                    "stado.pack",
                    "Remote node-source provisioning needs --app-repo <path>.",
                )
            })?;
            insert_source_input(&mut inputs, app_id, repository)?;
        }
        Some(Provision::AppBundle {
            bundle_path,
            bundle_name,
            ..
        }) => {
            if !bundle_path.exists() {
                return Err(Failure::config(
                    "stado.pack",
                    "The --app-bundle-path you gave does not exist. Build the bundle first.",
                ));
            }
            let (bundle, name) = pack_app_bundle(app_id, bundle_path)?;
            *bundle_name = Some(name);
            inputs.insert("bundle".into(), json!({
                "stado_uri": upload(&bundle.file, &format!("{app_id}-app-{}.tar.gz", bundle.hash))?,
                "relative_path": format!("inputs/{app_id}-app.tar.gz"),
            }));
            let repository = app_repo.ok_or_else(|| Failure::config(
                "stado.pack",
                format!("Remote app-bundle runs need the app source repo: pass --app-repo, or set repositories[0].root in apps/{app_id}/probierz.yaml."),
            ))?;
            insert_source_input(&mut inputs, app_id, repository)?;
        }
        None if source_required => {
            let repository = app_repo.ok_or_else(|| {
                Failure::config(
                    "stado.pack",
                    "Remote authoring needs the product source repository.",
                )
            })?;
            insert_source_input(&mut inputs, app_id, repository)?;
        }
        None => {}
    }
    Ok(inputs)
}

fn insert_source_input(inputs: &mut Map<String, Value>, app_id: &str, repository: &Path) -> Answer {
    let source = pack_app_source(app_id, repository)?;
    inputs.insert(
        "app".into(),
        json!({
            "stado_uri": upload(&source.file, &format!("{app_id}-{}.tar.gz", source.hash))?,
            "relative_path": format!("inputs/{app_id}.tar.gz"),
        }),
    );
    Ok(())
}

fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-'))
        && !value.split('/').any(|part| part == "..")
}

fn submit_machine(
    harness: &Path,
    selected: &discovery::Host,
    hash: &str,
    kind: &str,
    input_objects: Map<String, Value>,
    secret_env: Value,
    watch_budget_ms: u64,
) -> Result<Submission, Failure> {
    if watch_budget_ms == 0 {
        return Err(Failure::config(
            "stado.submit",
            "remote submission requires a positive watch budget",
        ));
    }
    let receipt_dir = harness
        .join("test-results")
        .join(".remote")
        .join(format!("probierz-{kind}-{hash}"));
    fs::create_dir_all(&receipt_dir)?;
    let request_file = receipt_dir.join("request.json");
    let mut request = Map::new();
    request.insert(
        "client_request_id".into(),
        Value::String(format!("probierz-{kind}-{hash}")),
    );
    request.insert(
        "command".into(),
        Value::String(format!(
            "{WATCH_BUDGET_ENV}={watch_budget_ms} bash inputs/run.sh"
        )),
    );
    request.insert("output_uri".into(), Value::String(state_uri("results")));
    request.insert("input_objects".into(), Value::Object(input_objects));
    request.insert("secret_env".into(), secret_env);
    if let Some(extra) = selected.request.as_ref().and_then(Value::as_object) {
        for (name, value) in extra {
            request.insert(name.clone(), value.clone());
        }
    }
    let request = Value::Object(request);
    write_json(&request_file, &request, false, false)?;
    eprintln!(
        "probierz-remote-request {}",
        json!({
            "requestId": request.get("client_request_id"),
            "requestFile": request_file,
        })
    );
    let submit = sh(
        STADO_BIN,
        &[
            "machine".into(),
            "submit".into(),
            "--request-file".into(),
            request_file.display().to_string(),
        ],
        None,
        Some(selected),
        None,
    );
    write_json(
        &receipt_dir.join("submission.json"),
        &process_record(&submit),
        true,
        false,
    )?;
    let payload: Option<Value> = serde_json::from_str(&submit.stdout).ok();
    let mut job_id = payload
        .as_ref()
        .filter(|value| value.get("ok").and_then(Value::as_bool) == Some(true))
        .and_then(|value| value.pointer("/result/job/job_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if job_id.is_none() {
        let message = payload
            .as_ref()
            .and_then(|value| value.pointer("/error/message"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if let Some(candidate) = submitted_job_id(message) {
            let status = sh(
                STADO_BIN,
                &["machine".into(), "status".into(), candidate.clone()],
                None,
                Some(selected),
                Some(STATUS_TIMEOUT),
            );
            if let Ok(value) = serde_json::from_str::<Value>(&status.stdout) {
                if value.get("ok").and_then(Value::as_bool) == Some(true)
                    && value.pointer("/result/job/job_id").and_then(Value::as_str)
                        == Some(candidate.as_str())
                {
                    job_id = Some(candidate);
                }
            }
        }
    }
    if let Some(job_id) = job_id {
        eprintln!(
            "probierz-remote-job {}",
            json!({
                "jobId": job_id,
                "requestId": request.get("client_request_id"),
                "receiptDir": receipt_dir,
            })
        );
        Ok(Submission {
            job_id: Some(job_id),
            watch_budget_ms,
            receipt_dir,
            failure: None,
        })
    } else {
        let failure = remote_failure(
            "stado.submit",
            "The stado queue did not accept the job",
            &submit,
        );
        Ok(Submission {
            job_id: None,
            watch_budget_ms,
            receipt_dir,
            failure: Some(failure_summary(
                &failure,
                "The stado queue did not accept the job.",
            )),
        })
    }
}

fn submitted_job_id(message: &str) -> Option<String> {
    let prefix = "job ";
    let suffix = " was submitted but its idempotency record could not be finalized:";
    let rest = message.strip_prefix(prefix)?;
    let (candidate, _) = rest.split_once(suffix)?;
    (candidate.len() >= 8 && candidate.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| candidate.to_string())
}

fn copy_submission_identity(
    identity: &Identity,
    provision: Option<&Provision>,
    receipt_dir: &Path,
    input_objects: &Map<String, Value>,
) -> Result<Map<String, Value>, Failure> {
    let source_identity_path = receipt_dir.join("source-identity.json");
    fs::copy(&identity.file, &source_identity_path)?;
    let source_revision = identity
        .document
        .pointer("/app/repositories")
        .and_then(Value::as_array)
        .and_then(|repositories| {
            repositories
                .iter()
                .find(|value| value.get("index").and_then(Value::as_u64) == Some(0))
        })
        .and_then(|value| value.get("gitSha"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut values = Map::new();
    values.insert(
        "receiptDir".into(),
        Value::String(receipt_dir.display().to_string()),
    );
    values.insert(
        "sourceIdentityPath".into(),
        Value::String(source_identity_path.display().to_string()),
    );
    values.insert(
        "sourceRevision".into(),
        source_revision
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    if let Some(Provision::NativeBinary {
        binary_name,
        binary_sha256,
        ..
    }) = provision
    {
        let binary = json!({
            "name": binary_name,
            "sha256": binary_sha256,
            "sourceRevision": source_revision,
            "input": input_objects.get("binary").and_then(|value| value.get("relative_path")).cloned().unwrap_or(Value::Null),
        });
        let binary_path = receipt_dir.join("binary-identity.json");
        write_json(&binary_path, &binary, true, true)?;
        values.insert("binary".into(), binary);
        values.insert(
            "binaryIdentityPath".into(),
            Value::String(binary_path.display().to_string()),
        );
    }
    Ok(values)
}

fn budget_from_job(job: &Value) -> Option<u64> {
    let command = job.get("command")?.as_str()?;
    let prefix = format!("{WATCH_BUDGET_ENV}=");
    command
        .split_ascii_whitespace()
        .find_map(|part| part.strip_prefix(&prefix))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
}

fn timestamp_millis(value: Option<&Value>) -> Option<i64> {
    value
        .and_then(Value::as_str)
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.timestamp_millis())
}

fn terminal_failure(job_id: &str, state: &str, job: &Value) -> Value {
    let reported = job.get("error").filter(|value| !value.is_null());
    let detail = if let Some(reported) = reported {
        reported
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| reported.to_string())
    } else if state == "cancelled" && job.get("started_at").map(Value::is_null).unwrap_or(true) {
        "cancelled before the worker started; no run evidence was produced".into()
    } else {
        format!("worker reported {state}")
    };
    let message = if state == "cancelled"
        && job.get("started_at").map(Value::is_null).unwrap_or(true)
    {
        format!("Job {job_id} was cancelled before a worker started; no run evidence was produced.")
    } else {
        format!("Job {job_id} {state} on the remote host: {detail}")
    };
    failure_summary(
        &Failure::new("stado.worker", Code::Unknown, detail),
        message,
    )
}

fn watch_job(
    harness: &Path,
    job_id: &str,
    selected: &discovery::Host,
    requested_budget: Option<u64>,
) -> Result<Value, Failure> {
    let now = Utc::now().timestamp_millis();
    let mut watch_budget = requested_budget;
    let mut budget_source = if requested_budget.is_some() {
        "submitted"
    } else {
        "original run"
    };
    let mut deadline = now.saturating_add(requested_budget.unwrap_or(SETUP_STEP_TIMEOUT_MS) as i64);
    let mut resolved_budget = false;
    let mut anchored_started_at = None;
    let mut failures = 0_usize;
    let mut last_job = Value::Null;
    let mut last_answered = false;
    loop {
        let current = Utc::now().timestamp_millis();
        if current >= deadline {
            break;
        }
        let remaining =
            Duration::from_millis((deadline - current).max(1) as u64).min(STATUS_TIMEOUT);
        let output = sh(
            STADO_BIN,
            &["machine".into(), "status".into(), job_id.into()],
            None,
            Some(selected),
            Some(remaining),
        );
        let payload: Option<Value> = serde_json::from_str(&output.stdout).ok();
        let answered = payload
            .as_ref()
            .and_then(|value| value.get("ok"))
            .and_then(Value::as_bool)
            == Some(true);
        last_answered = answered;
        let job = payload
            .as_ref()
            .and_then(|value| value.pointer("/result/job"))
            .cloned()
            .unwrap_or(Value::Null);
        if answered {
            last_job = job.clone();
        }
        let state = job
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        if answered
            && !resolved_budget
            && !matches!(
                state.as_str(),
                "failed" | "cancelled" | "completed" | "uploaded"
            )
        {
            let saved = budget_from_job(&job);
            watch_budget = saved.or(requested_budget);
            if watch_budget.is_none() {
                // Older submissions did not carry a budget.  A local authoring
                // receipt gives an exact application fallback without guessing.
                let app_id = read_author_submission(harness, job_id).and_then(|receipt| {
                    receipt
                        .get("appId")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
                watch_budget = match app_id {
                    Some(app_id) => Some(conservative_watch_budget(harness, &app_id)?),
                    None => None,
                };
            }
            let budget = watch_budget.ok_or_else(|| {
                Failure::config(
                    "stado.watch",
                    "The original run contract cannot be recovered from this job.",
                )
            })?;
            budget_source = if saved.is_some() {
                "saved submission"
            } else if requested_budget.is_some() {
                "submitted"
            } else {
                "original application fallback"
            };
            let created = timestamp_millis(job.get("created_at"))
                .unwrap_or_else(|| Utc::now().timestamp_millis());
            deadline = created.saturating_add(budget as i64);
            resolved_budget = true;
        }
        if answered {
            if let Some(started) = timestamp_millis(job.get("started_at")) {
                if anchored_started_at != Some(started) {
                    deadline = started
                        .saturating_add(watch_budget.unwrap_or(SETUP_STEP_TIMEOUT_MS) as i64);
                    anchored_started_at = Some(started);
                }
            }
            failures = 0;
        } else {
            failures += 1;
            if failures >= STATUS_FAILURE_TOLERANCE {
                let failure = remote_failure(
                    "stado.watch",
                    &format!("The stado queue stopped answering about job {job_id}"),
                    &output,
                );
                return Ok(json!({
                    "state": "unreachable",
                    "watchBudgetMs": watch_budget,
                    "failure": failure_summary(&failure, format!("The stado queue stopped answering about job {job_id}.")),
                }));
            }
        }
        if matches!(state.as_str(), "failed" | "cancelled") {
            let mut result = json!({
                "state": state,
                "source": job.pointer("/resolved_input_artifacts/source").cloned().unwrap_or(Value::Null),
                "job": job,
                "watchBudgetMs": watch_budget,
                "failure": terminal_failure(job_id, &state, &last_job),
            });
            if state == "cancelled"
                && last_job
                    .get("started_at")
                    .map(Value::is_null)
                    .unwrap_or(true)
            {
                result.as_object_mut().expect("object").insert("evidence".into(), json!({
                    "required": false, "collected": false, "reason": "cancelled-before-start", "retryable": false,
                }));
            }
            return Ok(result);
        }
        if matches!(state.as_str(), "uploaded" | "completed") {
            return Ok(json!({
                "state": "completed",
                "job": job,
                "source": last_job.pointer("/resolved_input_artifacts/source").cloned().unwrap_or(Value::Null),
                "watchBudgetMs": watch_budget,
                "failure": Value::Null,
            }));
        }
        let remaining = deadline - Utc::now().timestamp_millis();
        if remaining > 0 {
            thread::sleep(WATCH_INTERVAL.min(Duration::from_millis(remaining as u64)));
        }
    }
    let state = last_job
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("running")
        .to_ascii_lowercase();
    let budget = watch_budget.unwrap_or(SETUP_STEP_TIMEOUT_MS);
    let message = if last_answered {
        format!("Probierz stopped watching job {job_id} after its {budget}ms {budget_source} budget; Stado was still answering and the job remains {state}. Resume this job to continue waiting.")
    } else {
        format!("Probierz stopped watching job {job_id} after its {budget}ms {budget_source} budget; the last status read did not answer, but the queue-unreachable threshold was not reached. Resume this job to continue waiting.")
    };
    Ok(json!({
        "state": "watch-expired",
        "job": last_job,
        "source": last_job.pointer("/resolved_input_artifacts/source").cloned().unwrap_or(Value::Null),
        "watchBudgetMs": budget,
        "failure": failure_summary(&Failure::new("stado.watch", Code::Unknown, &message), message),
    }))
}

fn collection_directory(job_dir: &Path) -> Result<PathBuf, Failure> {
    fs::create_dir_all(job_dir)?;
    for sequence in 0..1000_u16 {
        let candidate = job_dir.join(format!("collection-{}-{sequence:03}", now_millis()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(Failure::config(
        "stado.download",
        "could not allocate a unique evidence collection directory",
    ))
}

fn fetch_run_evidence(
    harness: &Path,
    job_id: &str,
    selected: &discovery::Host,
) -> Result<Retained, Failure> {
    let job_dir = harness.join("test-results").join(".remote").join(job_id);
    fs::create_dir_all(&job_dir)?;
    let staging = work_path(&format!(
        "artifacts-{job_id}-{}-{}",
        now_millis(),
        std::process::id()
    ))?;
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    let output = sh(
        STADO_BIN,
        &[
            "machine".into(),
            "artifacts".into(),
            job_id.into(),
            "--output-dir".into(),
            staging.display().to_string(),
        ],
        None,
        Some(selected),
        None,
    );
    let payload: Value = match serde_json::from_str(&output.stdout) {
        Ok(value) => value,
        Err(_) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(remote_failure(
                "stado.download",
                "The queue returned invalid artifact metadata",
                &output,
            ));
        }
    };
    if output.status != Some(0) || payload.get("ok").and_then(Value::as_bool) != Some(true) {
        let upstream = payload.get("error").cloned();
        if upstream
            .as_ref()
            .and_then(|value| value.get("code"))
            .and_then(Value::as_str)
            == Some("NO_ARTIFACTS")
            && upstream
                .as_ref()
                .and_then(|value| value.get("retryable"))
                .and_then(Value::as_bool)
                == Some(false)
        {
            eprintln!(
                "probierz-remote-artifacts {}",
                json!({ "jobId": job_id, "error": upstream })
            );
            let _ = fs::remove_dir_all(&staging);
            return Ok(Retained {
                results_dir: None,
                manifest: None,
                author_receipt: None,
                author_receipt_file: None,
                artifact_error: upstream,
            });
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(remote_failure(
            "stado.download",
            "Downloading the worker's retained artifacts failed",
            &output,
        ));
    }
    let artifacts = payload
        .pointer("/result/artifacts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if artifacts.is_empty() {
        let _ = fs::remove_dir_all(&staging);
        return Ok(Retained {
            results_dir: None,
            manifest: None,
            author_receipt: None,
            author_receipt_file: None,
            artifact_error: None,
        });
    }
    let artifact_relative = artifacts
        .iter()
        .filter_map(|value| value.get("relative_path").and_then(Value::as_str))
        .find(|path| {
            (path.starts_with("probierz-run-")
                || path.starts_with("probierz-author-")
                || path.starts_with("probierz-seo-"))
                && path.ends_with(".tar.gz")
        })
        .map(str::to_string);
    let mut entries = Vec::new();
    if let Some(relative) = &artifact_relative {
        let tarball = safe_child(
            &staging,
            relative,
            "Remote evidence named an artifact outside its collection directory",
        )?;
        let listed = sh(
            "tar",
            &["-tzf".into(), tarball.display().to_string()],
            Some(harness),
            None,
            None,
        );
        if listed.status != Some(0) {
            let _ = fs::remove_dir_all(&staging);
            return Err(local_failure(
                "stado.download",
                "Listing the retained evidence archive failed",
                &listed,
            ));
        }
        entries = listed
            .stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        for entry in &entries {
            let normalized = entry.strip_prefix("./").unwrap_or(entry);
            if normalized != "test-results" && !normalized.starts_with("test-results/") {
                let _ = fs::remove_dir_all(&staging);
                return Err(Failure::config(
                    "stado.download",
                    format!("Remote evidence contains an unsafe retained path: {entry}"),
                ));
            }
            let _ = safe_child(
                harness,
                normalized,
                "Remote evidence contains an unsafe retained path",
            )?;
        }
    }
    let destination = collection_directory(&job_dir)?;
    fs::remove_dir(&destination)?;
    fs::rename(&staging, &destination)?;
    let Some(relative) = artifact_relative else {
        return Ok(Retained {
            results_dir: Some(destination),
            manifest: None,
            author_receipt: None,
            author_receipt_file: None,
            artifact_error: None,
        });
    };
    let tarball = safe_child(
        &destination,
        &relative,
        "Remote evidence named an artifact outside its collection directory",
    )?;
    let extracted = sh(
        "tar",
        &[
            "-xzf".into(),
            tarball.display().to_string(),
            "-C".into(),
            harness.display().to_string(),
        ],
        Some(harness),
        None,
        None,
    );
    if extracted.status != Some(0) {
        return Err(local_failure(
            "stado.download",
            "Extracting the retained evidence failed",
            &extracted,
        ));
    }
    let author_entry = entries
        .iter()
        .find(|entry| entry.ends_with("/accepted.json"));
    let author_receipt_file = author_entry.and_then(|entry| {
        safe_child(
            harness,
            entry.strip_prefix("./").unwrap_or(entry),
            "unsafe author receipt",
        )
        .ok()
    });
    let author_receipt = author_receipt_file.as_deref().and_then(read_json);
    let selected_run = author_receipt
        .as_ref()
        .and_then(|receipt| receipt.get("runId"))
        .and_then(Value::as_str);
    let mut run_manifest = None;
    for entry in entries
        .iter()
        .filter(|entry| entry.ends_with("/run-manifest.json"))
    {
        if let Ok(file) = safe_child(
            harness,
            entry.strip_prefix("./").unwrap_or(entry),
            "unsafe run manifest",
        ) {
            if let Some(candidate) = read_json(&file) {
                if selected_run
                    .map(|run_id| candidate.get("runId").and_then(Value::as_str) == Some(run_id))
                    .unwrap_or(true)
                {
                    run_manifest = Some(candidate);
                    break;
                }
            }
        }
    }
    Ok(Retained {
        results_dir: Some(destination),
        manifest: run_manifest,
        author_receipt,
        author_receipt_file,
        artifact_error: None,
    })
}

fn safe_child(root: &Path, relative: &str, message: &str) -> Result<PathBuf, Failure> {
    if relative.is_empty()
        || Path::new(relative).is_absolute()
        || Path::new(relative)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(Failure::config(
            "stado.download",
            format!("{message}: {relative}"),
        ));
    }
    Ok(root.join(relative))
}

fn read_json(path: &Path) -> Option<Value> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn missing_evidence(job_id: &str, detail: &str) -> Value {
    failure_summary(
        &Failure::config("stado.download", format!("job={job_id}; {detail}")),
        format!("Job {job_id} completed without the required Probierz run evidence"),
    )
}

fn read_author_submission(harness: &Path, job_id: &str) -> Option<Value> {
    let file = harness
        .join("test-results")
        .join(".remote")
        .join(job_id)
        .join("authoring-submission.json");
    let value = read_json(&file)?;
    (value.get("schemaVersion").and_then(Value::as_u64) == Some(1)
        && value.get("jobId").and_then(Value::as_str) == Some(job_id))
    .then_some(value)
}

fn save_author_submission(
    harness: &Path,
    job_id: &str,
    app_id: &str,
    journey: &str,
    area: &str,
    target: &str,
    product_root: &Path,
    test_directory: &str,
    identity: &Identity,
) -> Result<PathBuf, Failure> {
    let file = harness
        .join("test-results")
        .join(".remote")
        .join(job_id)
        .join("authoring-submission.json");
    let value = json!({
        "schemaVersion": 1,
        "jobId": job_id,
        "appId": app_id,
        "journey": journey,
        "area": area,
        "target": target,
        "productRoot": product_root,
        "testDirectory": test_directory,
        "sourceSha256": identity.document.pointer("/app/sha256").cloned().unwrap_or(Value::Null),
        "harnessSha256": identity.document.pointer("/harness/sha256").cloned().unwrap_or(Value::Null),
        "installedSourceSha256": Value::Null,
    });
    write_json(&file, &value, true, true)?;
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

fn restore_remote_authoring(
    harness: &Path,
    job_id: &str,
    retained: &Retained,
    expected_app: Option<&str>,
    required: bool,
) -> Result<Option<Value>, Failure> {
    let Some(receipt) = retained.author_receipt.as_ref() else {
        if required {
            return Err(Failure::config(
                "stado.download",
                format!("Job {job_id} completed authoring without a usable accepted-spec receipt."),
            ));
        }
        return Ok(None);
    };
    let submission = read_author_submission(harness, job_id).ok_or_else(|| Failure::config(
        "stado.download",
        format!("Job {job_id} returned an authored spec, but this checkout has no source-bound submission receipt."),
    ))?;
    let app_id = submission
        .get("appId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let journey = submission
        .get("journey")
        .and_then(Value::as_str)
        .unwrap_or("");
    let area = submission.get("area").and_then(Value::as_str).unwrap_or("");
    let target = submission
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or("");
    let product_root = PathBuf::from(
        submission
            .get("productRoot")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let test_directory = submission
        .get("testDirectory")
        .and_then(Value::as_str)
        .unwrap_or("tests");
    let expected_spec = format!(
        "{test_directory}/{area}/{journey}.probierz.spec.{}",
        product_extension(target)
    );
    let expected_registration = registration_directory(target)
        .map(|directory| {
            format!(
                "{directory}/{app_id}-{journey}{}",
                registration_extension(target)
            )
        })
        .unwrap_or_default();
    let local_application = manifest::load(harness, app_id)?;
    let local_test_directory = local_application
        .document
        .get("surfaces")
        .and_then(|value| value.get(target))
        .and_then(|value| value.get("testDirectory"))
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("tests");
    let manifest = retained.manifest.as_ref();
    let matching = receipt.get("schemaVersion").and_then(Value::as_u64) == Some(1)
        && receipt.get("appId").and_then(Value::as_str) == Some(app_id)
        && receipt.get("journey").and_then(Value::as_str) == Some(journey)
        && receipt.get("area").and_then(Value::as_str) == Some(area)
        && receipt.get("target").and_then(Value::as_str) == Some(target)
        && local_test_directory == test_directory
        && !expected_registration.is_empty()
        && receipt
            .pointer("/spec/relativePath")
            .and_then(Value::as_str)
            == Some(expected_spec.as_str())
        && receipt
            .pointer("/registration/relativePath")
            .and_then(Value::as_str)
            == Some(expected_registration.as_str())
        && receipt
            .get("mappingPaths")
            .and_then(Value::as_array)
            .map(Vec::is_empty)
            == Some(true)
        && manifest.and_then(|value| value.get("runId")) == receipt.get("runId")
        && manifest
            .and_then(|value| value.get("appId"))
            .and_then(Value::as_str)
            == Some(app_id)
        && manifest
            .and_then(|value| value.get("target"))
            .and_then(Value::as_str)
            == Some(target)
        && manifest.and_then(|value| value.pointer("/source/sha256"))
            == submission.get("sourceSha256")
        && manifest.and_then(|value| value.pointer("/harness/sha256"))
            == submission.get("harnessSha256")
        && manifest
            .and_then(|value| value.get("sourceIdentityOrigin"))
            .and_then(Value::as_str)
            == Some("submitter")
        && manifest
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            == Some("passed")
        && expected_app
            .map(|expected| expected == app_id)
            .unwrap_or(true);
    if !matching {
        return Err(Failure::config(
            "stado.download",
            format!("Job {job_id} returned authoring metadata that does not match its submitting checkout."),
        ));
    }
    let source_sha = submission.get("sourceSha256").and_then(Value::as_str);
    let harness_sha = submission.get("harnessSha256").and_then(Value::as_str);
    if source_sha.is_none()
        || receipt.get("sourceSha256").and_then(Value::as_str) != source_sha
        || harness_sha.is_none()
        || receipt.get("harnessSha256").and_then(Value::as_str) != harness_sha
    {
        return Err(Failure::config(
            "stado.download",
            format!("Job {job_id} returned an authored spec for a different source identity."),
        ));
    }
    let current_before =
        crate::authoring::app_source_identity(harness, app_id, Some(&product_root))?;
    let expected_local = submission
        .get("installedSourceSha256")
        .and_then(Value::as_str)
        .or(source_sha);
    if current_before
        .pointer("/app/sha256")
        .and_then(Value::as_str)
        .is_none()
        || current_before
            .pointer("/app/sha256")
            .and_then(Value::as_str)
            != expected_local
    {
        return Err(Failure::config(
            "stado.download",
            format!("Job {job_id} cannot publish into a checkout whose source changed after submission."),
        ));
    }
    let accepted =
        receipt
            .pointer("/spec/artifact")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Failure::config(
        "stado.download",
        format!("Job {job_id} returned an authored spec outside retained Probierz artifacts."),
    )
            })?;
    let accepted = safe_child(
        harness,
        accepted,
        "authored spec outside retained Probierz artifacts",
    )?;
    let results_root = harness.join("test-results");
    if !accepted.starts_with(&results_root) || !accepted.is_file() {
        return Err(Failure::config(
            "stado.download",
            format!("Job {job_id} returned an authored spec outside retained Probierz artifacts."),
        ));
    }
    let bytes = fs::read(&accepted)?;
    let digest = hex::encode(Sha256::digest(&bytes));
    if receipt.pointer("/spec/bytes").and_then(Value::as_u64) != Some(bytes.len() as u64)
        || receipt.pointer("/spec/sha256").and_then(Value::as_str) != Some(digest.as_str())
    {
        return Err(Failure::config(
            "stado.download",
            format!("Job {job_id} returned authored spec bytes that do not match its receipt."),
        ));
    }
    let installed = install_product_spec(
        harness,
        &product_root,
        app_id,
        journey,
        target,
        &expected_spec,
        &expected_registration,
        &bytes,
    )?;
    let current = crate::authoring::app_source_identity(harness, app_id, Some(&product_root))?;
    let source = current
        .pointer("/app/sha256")
        .cloned()
        .unwrap_or(Value::Null);
    if source.is_null() {
        return Err(Failure::config(
            "stado.download",
            format!("Job {job_id} installed an authored spec, but its product source identity is unavailable."),
        ));
    }
    let mut updated = submission.clone();
    if let Some(object) = updated.as_object_mut() {
        object.insert("installedSourceSha256".into(), source);
    }
    let source_receipt = harness
        .join("test-results")
        .join(".remote")
        .join(job_id)
        .join("authoring-submission.json");
    write_json(&source_receipt, &updated, true, true)?;
    Ok(Some(json!({
        "productSpec": installed.0,
        "registration": installed.1,
        "appManifest": installed.2,
        "authorReceipt": retained.author_receipt_file,
        "sourceReceipt": source_receipt,
    })))
}

fn registration_directory(target: &str) -> Option<&'static str> {
    match target {
        "web" => Some("packages/web/tests"),
        "electron" => Some("packages/electron/tests"),
        "mobile:ios" | "mobile:android" => Some("packages/mobile/test/specs"),
        "desktop:mac" | "desktop:win" => Some("packages/desktop-native/test/specs"),
        "desktop:cua" => Some("packages/desktop-cua/specs"),
        "tui" => Some("packages/tui/specs"),
        _ => None,
    }
}

fn registration_extension(target: &str) -> &'static str {
    if matches!(target, "web" | "electron") {
        ".spec.ts"
    } else if matches!(target, "tui" | "desktop:cua") {
        ".spec.mjs"
    } else {
        ".e2e.ts"
    }
}

fn product_extension(target: &str) -> &'static str {
    if matches!(target, "tui" | "desktop:cua") {
        "mjs"
    } else {
        "ts"
    }
}

fn authored_path_is_inside(root: &Path, candidate: &Path) -> bool {
    candidate != root && candidate.starts_with(root)
}

fn relative_symlink_target(from: &Path, to: &Path) -> PathBuf {
    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();
    let mut shared = 0;
    while shared < from_components.len()
        && shared < to_components.len()
        && from_components[shared] == to_components[shared]
    {
        shared += 1;
    }
    if shared == 0 {
        return to.to_path_buf();
    }
    let mut relative = PathBuf::new();
    for component in &from_components[shared..] {
        if matches!(component, std::path::Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &to_components[shared..] {
        relative.push(component.as_os_str());
    }
    relative
}

fn assert_physical_product_path(
    product_root: &Path,
    tests_root: &Path,
    product_spec: &Path,
) -> Result<(), Failure> {
    match fs::symlink_metadata(product_spec) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(Failure::config(
                "stado.download",
                "authored spec destination must be a regular product file",
            ));
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let parent = product_spec.parent().ok_or_else(|| {
        Failure::config(
            "stado.download",
            "authored spec path escapes the selected product tests directory",
        )
    })?;
    let mut existing_parent = parent;
    while !existing_parent.exists() {
        existing_parent = existing_parent.parent().ok_or_else(|| {
            Failure::config(
                "stado.download",
                "authored spec path escapes the selected product tests directory",
            )
        })?;
    }
    let physical_root = fs::canonicalize(product_root)?;
    let physical_parent = fs::canonicalize(existing_parent)?;
    if physical_parent != physical_root
        && !authored_path_is_inside(&physical_root, &physical_parent)
    {
        return Err(Failure::config(
            "stado.download",
            "authored spec path escapes the selected product tests directory",
        ));
    }
    if tests_root.exists() && parent.exists() {
        let physical_tests = fs::canonicalize(tests_root)?;
        let physical_product =
            fs::canonicalize(parent)?.join(product_spec.file_name().ok_or_else(|| {
                Failure::config(
                    "stado.download",
                    "authored spec path escapes the selected product tests directory",
                )
            })?);
        if !authored_path_is_inside(&physical_root, &physical_tests)
            || !authored_path_is_inside(&physical_tests, &physical_product)
        {
            return Err(Failure::config(
                "stado.download",
                "authored spec path escapes the selected product tests directory",
            ));
        }
    }
    Ok(())
}

fn install_product_spec(
    harness: &Path,
    product_root: &Path,
    app_id: &str,
    journey: &str,
    target: &str,
    product_relative: &str,
    registration_relative: &str,
    bytes: &[u8],
) -> Result<(PathBuf, PathBuf, PathBuf), Failure> {
    for relative in [product_relative, registration_relative] {
        let path = Path::new(relative);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(Failure::config(
                "stado.download",
                "Remote authoring returned an unsafe local installation path.",
            ));
        }
    }
    let loaded = manifest::load(harness, app_id)?;
    let manifest_file = loaded.file;
    let mut document = loaded.document;
    let owner = document
        .get("owner")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("probierz")
        .to_string();
    let journeys = document
        .get_mut("journeys")
        .and_then(serde_yaml::Value::as_mapping_mut)
        .ok_or_else(|| Failure::config("stado.download", "manifest journeys are required"))?;
    journeys
        .entry(serde_yaml::Value::from(journey))
        .or_insert_with(|| {
            serde_yaml::to_value(json!({ "owner": owner, "timeoutMs": 300000 }))
                .unwrap_or(serde_yaml::Value::Null)
        });
    let declared = document
        .get_mut("surfaces")
        .and_then(serde_yaml::Value::as_mapping_mut)
        .and_then(|surfaces| surfaces.get_mut(serde_yaml::Value::from(target)))
        .and_then(|surface| surface.get_mut("journeys"))
        .and_then(serde_yaml::Value::as_sequence_mut)
        .ok_or_else(|| {
            Failure::config(
                "stado.download",
                format!("app {app_id} has no {target} surface"),
            )
        })?;
    if !declared.iter().any(|value| value.as_str() == Some(journey)) {
        declared.push(serde_yaml::Value::from(journey));
        declared.sort_by(|left, right| {
            left.as_str()
                .unwrap_or_default()
                .cmp(right.as_str().unwrap_or_default())
        });
    }
    manifest::validate(&document, &manifest_file)?;

    let product_spec = product_root.join(product_relative);
    let tests_root = product_spec
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            Failure::config(
                "stado.download",
                "authored spec path escapes the selected product tests directory",
            )
        })?;
    assert_physical_product_path(product_root, tests_root, &product_spec)?;
    let registration = harness.join(registration_relative);
    let replaced_product = fs::symlink_metadata(&registration)
        .ok()
        .filter(|metadata| metadata.file_type().is_symlink())
        .and_then(|_| fs::read_link(&registration).ok())
        .and_then(|target| {
            let candidate = if target.is_absolute() {
                target
            } else {
                registration.parent().unwrap_or(harness).join(target)
            };
            fs::canonicalize(candidate).ok()
        })
        .filter(|candidate| {
            fs::canonicalize(&product_spec).ok().as_ref() != Some(candidate)
                && fs::canonicalize(tests_root)
                    .ok()
                    .is_some_and(|tests| authored_path_is_inside(&tests, candidate))
        });
    if let Some(parent) = product_spec.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = registration.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&product_spec, bytes)?;
    if let Some(replaced) = replaced_product {
        fs::remove_file(replaced)?;
    }
    if fs::symlink_metadata(&registration).is_ok() {
        fs::remove_file(&registration)?;
    }
    let link_target =
        relative_symlink_target(registration.parent().unwrap_or(harness), &product_spec);
    std::os::unix::fs::symlink(link_target, &registration)?;
    fs::write(&manifest_file, serde_yaml::to_string(&document)?)?;
    Ok((product_spec, registration, manifest_file))
}

fn collect_remote_run(
    harness: &Path,
    job_id: Option<&str>,
    app_id: &str,
    host_name: &str,
) -> Result<Value, Failure> {
    let job_id = job_id.unwrap_or("");
    let selected = discovery::stado_host(host_name);
    if !canonical_job_id(job_id) || selected.is_none() {
        return Err(Failure::config(
            "stado.download",
            "Collection requires a canonical Stado job ID and a known Stado host.",
        ));
    }
    let selected = selected.expect("checked");
    manifest::load(harness, app_id)?;
    let status = sh(
        STADO_BIN,
        &["machine".into(), "status".into(), job_id.into()],
        None,
        Some(&selected),
        Some(STATUS_TIMEOUT),
    );
    if status.status != Some(0) {
        return Err(remote_failure(
            "stado.watch",
            &format!("Reading job {job_id} failed"),
            &status,
        ));
    }
    let payload: Value = serde_json::from_str(&status.stdout).map_err(|_| {
        remote_failure(
            "stado.watch",
            &format!("Reading job {job_id} returned invalid status"),
            &status,
        )
    })?;
    let job = payload
        .pointer("/result/job")
        .cloned()
        .unwrap_or(Value::Null);
    let state = job
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if payload.get("ok").and_then(Value::as_bool) != Some(true) || state.is_empty() {
        return Err(remote_failure(
            "stado.watch",
            &format!("Reading job {job_id} returned no state"),
            &status,
        ));
    }
    let mut result = json!({
        "host": host_name,
        "jobId": job_id,
        "appId": app_id,
        "state": state,
        "submitted": true,
        "collected": false,
        "job": job,
        "source": job.pointer("/resolved_input_artifacts/source").cloned().unwrap_or(Value::Null),
    });
    if !matches!(
        state.as_str(),
        "uploaded" | "completed" | "failed" | "cancelled"
    ) {
        return Ok(result);
    }
    if state == "cancelled" && job.get("started_at").map(Value::is_null).unwrap_or(true) {
        let object = result.as_object_mut().expect("object");
        object.insert("failure".into(), terminal_failure(job_id, &state, &job));
        object.insert("evidence".into(), json!({
            "required": false, "collected": false, "reason": "cancelled-before-start", "retryable": false,
        }));
        return Ok(result);
    }
    let retained = fetch_run_evidence(harness, job_id, &selected)?;
    let manifest_matches = retained
        .manifest
        .as_ref()
        .and_then(|value| value.get("appId"))
        .and_then(Value::as_str)
        == Some(app_id);
    if !manifest_matches {
        let terminal = if state == "uploaded" {
            "completed"
        } else {
            state.as_str()
        };
        let object = result.as_object_mut().expect("object");
        object.insert(
            "state".into(),
            Value::String(
                if terminal == "completed" {
                    "evidence-unavailable"
                } else {
                    terminal
                }
                .to_string(),
            ),
        );
        object.insert(
            "artifactError".into(),
            retained.artifact_error.clone().unwrap_or(Value::Null),
        );
        object.insert(
            "failure".into(),
            if terminal == "completed" {
                missing_evidence(
                    job_id,
                    &format!(
                        "app={app_id}; artifact_error={}",
                        retained.artifact_error.as_ref().unwrap_or(&Value::Null)
                    ),
                )
            } else {
                terminal_failure(job_id, terminal, &job)
            },
        );
        return Ok(result);
    }
    let authored = restore_remote_authoring(
        harness,
        job_id,
        &retained,
        Some(app_id),
        matches!(state.as_str(), "uploaded" | "completed")
            && read_author_submission(harness, job_id).is_some(),
    )?;
    let object = result.as_object_mut().expect("object");
    object.insert(
        "state".into(),
        Value::String(if state == "uploaded" {
            "completed".into()
        } else {
            state
        }),
    );
    object.insert("collected".into(), Value::Bool(true));
    object.insert(
        "resultsDir".into(),
        retained
            .results_dir
            .map(|path| Value::String(path.display().to_string()))
            .unwrap_or(Value::Null),
    );
    object.insert("manifest".into(), retained.manifest.unwrap_or(Value::Null));
    if let Some(Value::Object(authored)) = authored {
        object.extend(authored);
    }
    Ok(result)
}

fn capture_remote_logs(
    job_id: &str,
    selected: &discovery::Host,
    directory: &Path,
) -> Result<(PathBuf, PathBuf, Option<Value>), Failure> {
    let log_path = directory.join("command.log");
    let receipts = directory.join("log-receipts.jsonl");
    fs::write(&log_path, [])?;
    fs::write(&receipts, [])?;
    let mut cursor = 0_u64;
    loop {
        let page = sh(
            STADO_BIN,
            &[
                "machine".into(),
                "logs".into(),
                job_id.into(),
                "--cursor".into(),
                cursor.to_string(),
                "--limit".into(),
                "65536".into(),
            ],
            None,
            Some(selected),
            Some(STATUS_TIMEOUT),
        );
        append_line(&receipts, page.stdout.trim())?;
        let payload: Value = match serde_json::from_str(&page.stdout) {
            Ok(value) => value,
            Err(_) => {
                let failure = remote_failure(
                    "stado.download",
                    &format!("Reading logs for job {job_id} returned invalid metadata"),
                    &page,
                );
                return Ok((
                    log_path,
                    receipts,
                    Some(failure_summary(
                        &failure,
                        format!("Reading logs for job {job_id} returned invalid metadata."),
                    )),
                ));
            }
        };
        if page.status != Some(0) || payload.get("ok").and_then(Value::as_bool) != Some(true) {
            let failure = remote_failure(
                "stado.download",
                &format!("Reading logs for job {job_id} failed"),
                &page,
            );
            return Ok((
                log_path,
                receipts,
                Some(failure_summary(
                    &failure,
                    format!("Reading logs for job {job_id} failed."),
                )),
            ));
        }
        let text = payload
            .pointer("/result/text")
            .and_then(Value::as_str)
            .unwrap_or("");
        OpenOptions::new()
            .append(true)
            .open(&log_path)?
            .write_all(text.as_bytes())?;
        if payload.pointer("/result/eof").and_then(Value::as_bool) == Some(true) {
            return Ok((log_path, receipts, None));
        }
        let next = payload
            .pointer("/result/next_cursor")
            .and_then(Value::as_u64);
        if next.map(|value| value > cursor) != Some(true) {
            let message = format!("Stado returned an invalid log cursor for job {job_id}; the pages received so far were retained.");
            let failure = Failure::new("stado.download", Code::Unknown, &message);
            return Ok((log_path, receipts, Some(failure_summary(&failure, message))));
        }
        cursor = next.expect("checked");
    }
}

fn append_line(path: &Path, text: &str) -> Answer {
    let mut file = OpenOptions::new().append(true).open(path)?;
    file.write_all(text.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

fn cancel_remote_run(
    harness: &Path,
    job_id: Option<&str>,
    host_name: &str,
    reason: &str,
) -> Result<Value, Failure> {
    let job_id = job_id.unwrap_or("");
    if !canonical_job_id(job_id) {
        return Err(Failure::config(
            "stado.watch",
            "Cancelling remote evidence needs a canonical Stado job ID.",
        ));
    }
    let selected = host(host_name, "stado.watch")?;
    let reason = reason.trim();
    if reason.is_empty() || reason.contains('\0') {
        return Err(Failure::config(
            "stado.watch",
            "Cancelling a remote run needs --reason <reason>.",
        ));
    }
    let requested = Utc::now();
    let attempt_id = format!(
        "{}-{}",
        requested.format("%Y%m%d%H%M%S%3f"),
        &nonce("cancel")[..8]
    );
    let cancellation_root = harness
        .join("test-results")
        .join(".remote")
        .join("cancellations")
        .join(job_id);
    let directory = cancellation_root.join(&attempt_id);
    fs::create_dir_all(&directory)?;
    let request_path = directory.join("request.json");
    write_json(
        &request_path,
        &json!({
            "schemaVersion": 1,
            "jobId": job_id,
            "host": host_name,
            "reason": reason,
            "requestedAt": requested.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        }),
        true,
        true,
    )?;
    let before = sh(
        STADO_BIN,
        &["machine".into(), "status".into(), job_id.into()],
        None,
        Some(&selected),
        Some(STATUS_TIMEOUT),
    );
    let before_path = directory.join("status-before.json");
    fs::write(&before_path, &before.stdout)?;
    write_json(
        &directory.join("status-before-process.json"),
        &process_record(&before),
        true,
        true,
    )?;
    let before_payload: Value = serde_json::from_str(&before.stdout).map_err(|_| {
        remote_failure(
            "stado.watch",
            &format!("Reading the original state for job {job_id} returned invalid metadata"),
            &before,
        )
    })?;
    let original_job = before_payload
        .pointer("/result/job")
        .cloned()
        .unwrap_or(Value::Null);
    if before.status != Some(0)
        || before_payload.get("ok").and_then(Value::as_bool) != Some(true)
        || original_job.is_null()
    {
        return Err(remote_failure(
            "stado.watch",
            &format!("Reading the original state for job {job_id} failed"),
            &before,
        ));
    }
    let cancellation = sh(
        STADO_BIN,
        &["machine".into(), "cancel".into(), job_id.into()],
        None,
        Some(&selected),
        Some(STATUS_TIMEOUT),
    );
    let receipt_path = directory.join("receipt.json");
    fs::write(&receipt_path, &cancellation.stdout)?;
    write_json(
        &directory.join("receipt-process.json"),
        &process_record(&cancellation),
        true,
        true,
    )?;
    let cancellation_payload: Value = serde_json::from_str(&cancellation.stdout).map_err(|_| {
        remote_failure(
            "stado.watch",
            &format!("Cancelling job {job_id} returned an invalid receipt"),
            &cancellation,
        )
    })?;
    let job = cancellation_payload
        .pointer("/result/job")
        .cloned()
        .unwrap_or(Value::Null);
    if cancellation.status != Some(0)
        || cancellation_payload.get("ok").and_then(Value::as_bool) != Some(true)
        || job.is_null()
    {
        return Err(remote_failure(
            "stado.watch",
            &format!("Cancelling job {job_id} failed"),
            &cancellation,
        ));
    }
    let (log_path, log_receipts, log_failure) = capture_remote_logs(job_id, &selected, &directory)?;
    let state = job
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut retained = None;
    let mut evidence_failure = None;
    if job.get("started_at").is_some_and(|value| !value.is_null())
        && matches!(
            state.as_str(),
            "cancelled" | "completed" | "uploaded" | "failed"
        )
    {
        match fetch_run_evidence(harness, job_id, &selected) {
            Ok(value) => retained = Some(value),
            Err(failure) => {
                evidence_failure = Some(failure_summary(&failure, failure.detail.clone()))
            }
        }
    }
    let cancelled = state == "cancelled";
    let required = job.get("started_at").is_some_and(|value| !value.is_null());
    let collected = retained
        .as_ref()
        .and_then(|value| value.results_dir.as_ref())
        .is_some();
    let artifact_error = retained
        .as_ref()
        .and_then(|value| value.artifact_error.clone());
    let evidence = json!({
        "required": required,
        "collected": collected,
        "resultsDir": retained.as_ref().and_then(|value| value.results_dir.as_ref()).map(|path| path.display().to_string()),
        "artifactError": artifact_error,
        "failure": evidence_failure,
        "reason": if required { Value::Null } else { Value::String("cancelled-before-start".into()) },
    });
    let evaluation_failure = if cancelled {
        terminal_failure(job_id, &state, &job)
    } else {
        let message = format!(
            "Job {job_id} is {} and was not cancelled.",
            if state.is_empty() {
                "in an unknown state"
            } else {
                &state
            }
        );
        failure_summary(
            &Failure::new("stado.worker", Code::Unknown, &message),
            message,
        )
    };
    let cancellation_failure = if !cancelled {
        Some(evaluation_failure.clone())
    } else if let Some(value) = log_failure.clone().or(evidence_failure.clone()) {
        Some(value)
    } else if required && !collected {
        let message = format!("Cancellation of job {job_id} succeeded, but its required worker evidence was not retained.");
        Some(failure_summary(
            &Failure::config("stado.download", &message),
            message,
        ))
    } else {
        None
    };
    Ok(json!({
        "host": host_name,
        "jobId": job_id,
        "submitted": false,
        "state": state,
        "cancelled": cancelled,
        "passed": false,
        "cancellationSucceeded": cancelled && cancellation_failure.is_none(),
        "cancellationFailure": cancellation_failure,
        "reason": reason,
        "cancellationRoot": cancellation_root,
        "attemptId": attempt_id,
        "originalJob": original_job,
        "job": job,
        "source": original_job.pointer("/resolved_input_artifacts/source").cloned()
            .or_else(|| job.pointer("/resolved_input_artifacts/source").cloned()).unwrap_or(Value::Null),
        "cancellationDir": directory,
        "requestPath": request_path,
        "statusBeforePath": before_path,
        "receiptPath": receipt_path,
        "logsPath": log_path,
        "logReceiptsPath": log_receipts,
        "logFailure": log_failure,
        "evidence": evidence,
        "resultsDir": retained.and_then(|value| value.results_dir).map(|path| path.display().to_string()),
        "failure": evaluation_failure,
    }))
}

fn resume_remote_run(
    harness: &Path,
    job_id: Option<&str>,
    host_name: &str,
) -> Result<Value, Failure> {
    let job_id = job_id.unwrap_or("");
    if !safe_job_identifier(job_id) {
        return Err(Failure::config(
            "stado.watch",
            "Resuming remote evidence needs a valid existing Stado job ID.",
        ));
    }
    let selected = host(host_name, "stado.watch")?;
    let watched = watch_job(harness, job_id, &selected, None)?;
    let mut result = json!({
        "host": host_name,
        "jobId": job_id,
        "submitted": false,
    });
    result
        .as_object_mut()
        .expect("object")
        .extend(watched.as_object().cloned().unwrap_or_default());
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !matches!(state.as_str(), "completed" | "failed") {
        return Ok(result);
    }
    let retained = fetch_run_evidence(harness, job_id, &selected)?;
    if let Some(path) = &retained.results_dir {
        result.as_object_mut().expect("object").insert(
            "resultsDir".into(),
            Value::String(path.display().to_string()),
        );
    }
    if let Some(error) = &retained.artifact_error {
        result
            .as_object_mut()
            .expect("object")
            .insert("artifactError".into(), error.clone());
    }
    if let Some(run) = &retained.manifest {
        let object = result.as_object_mut().expect("object");
        for (output, input) in [("runId", "runId"), ("appId", "appId"), ("target", "target")] {
            object.insert(
                output.into(),
                run.get(input).cloned().unwrap_or(Value::Null),
            );
        }
        let authored = restore_remote_authoring(
            harness,
            job_id,
            &retained,
            run.get("appId").and_then(Value::as_str),
            state == "completed" && read_author_submission(harness, job_id).is_some(),
        )?;
        if let Some(Value::Object(authored)) = authored {
            object.extend(authored);
        }
    } else if state == "completed" {
        let object = result.as_object_mut().expect("object");
        object.insert("state".into(), Value::String("evidence-unavailable".into()));
        object.insert(
            "failure".into(),
            missing_evidence(
                job_id,
                &format!(
                    "artifact_error={}",
                    retained.artifact_error.as_ref().unwrap_or(&Value::Null),
                ),
            ),
        );
    }
    Ok(result)
}

fn canonical_job_id(value: &str) -> bool {
    value.len() == 28
        && value.starts_with("job-")
        && value[4..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn safe_job_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn submit_remote_run(
    harness: &Path,
    target: &str,
    app_id: &str,
    spec: Option<&str>,
    host_name: &str,
    mut provision: Option<Provision>,
    app_repo: Option<&Path>,
    watch: bool,
    mode: &str,
    record: bool,
    environment: &[(String, String)],
) -> Result<Value, Failure> {
    let selected = host(host_name, "stado.submit")?;
    require_gui_ready(target, &selected)?;
    let identity = pack_source_identity(harness, app_id, app_repo)?;
    require_immutable_native(target, provision.as_ref(), app_repo, &identity)?;
    let watch_budget =
        selected_run_budget(harness, app_id, target, environment, provision.as_ref())?;
    let packed = pack_repo(harness, &[app_id])?;
    let repo_uri = upload(&packed.file, &format!("probierz-{}.tar.gz", packed.hash))?;
    let identity_uri = upload(
        &identity.file,
        &format!("source-{app_id}-{}.json", identity.hash),
    )?;
    let provisioned = provision_inputs(app_id, &mut provision, app_repo, false)?;
    let script = run_script(
        target,
        app_id,
        spec,
        provision.as_ref(),
        &packed.hash,
        selected.platform,
        mode,
        None,
        None,
        record,
        environment,
    )?;
    let script_file = work_path(&format!("probierz-run-{}.sh", packed.hash))?;
    fs::write(&script_file, script)?;
    let script_uri = upload(&script_file, &format!("run-{}.sh", packed.hash))?;
    let mut inputs = Map::new();
    inputs.insert(
        "repo".into(),
        json!({ "stado_uri": repo_uri, "relative_path": "inputs/probierz.tar.gz" }),
    );
    inputs.insert(
        "script".into(),
        json!({ "stado_uri": script_uri, "relative_path": "inputs/run.sh" }),
    );
    inputs.insert(
        "source".into(),
        json!({ "stado_uri": identity_uri, "relative_path": "inputs/source-identity.json" }),
    );
    inputs.extend(provisioned);
    let secrets = remote_secret_env(
        harness,
        app_id,
        &["STADO_MODEL_ROUTER_TOKEN", "PROBIERZ_MODEL_AGENT_SECRET"],
    )?;
    let submission = submit_machine(
        harness,
        &selected,
        &packed.hash,
        "run",
        inputs.clone(),
        secrets,
        watch_budget,
    )?;
    let identity_fields = copy_submission_identity(
        &identity,
        provision.as_ref(),
        &submission.receipt_dir,
        &inputs,
    )?;
    let mut result = json!({
        "host": host_name,
        "jobId": submission.job_id,
        "target": target,
        "appId": app_id,
        "submitted": submission.job_id.is_some(),
        "watchBudgetMs": submission.watch_budget_ms,
    });
    result
        .as_object_mut()
        .expect("object")
        .extend(identity_fields);
    let Some(job_id) = submission.job_id else {
        let object = result.as_object_mut().expect("object");
        object.insert("state".into(), Value::String("submit-failed".into()));
        object.insert("failure".into(), submission.failure.unwrap_or(Value::Null));
        return Ok(result);
    };
    if !watch {
        let object = result.as_object_mut().expect("object");
        object.insert("state".into(), Value::String("queued".into()));
        object.insert("failure".into(), Value::Null);
        return Ok(result);
    }
    let watched = watch_job(harness, &job_id, &selected, Some(watch_budget))?;
    result
        .as_object_mut()
        .expect("object")
        .extend(watched.as_object().cloned().unwrap_or_default());
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if matches!(state.as_str(), "completed" | "failed") {
        let retained = fetch_run_evidence(harness, &job_id, &selected)?;
        if let Some(path) = retained.results_dir {
            result.as_object_mut().expect("object").insert(
                "resultsDir".into(),
                Value::String(path.display().to_string()),
            );
        }
        if let Some(error) = retained.artifact_error {
            result
                .as_object_mut()
                .expect("object")
                .insert("artifactError".into(), error);
        }
        if state == "completed" && result.get("resultsDir").is_none() {
            let object = result.as_object_mut().expect("object");
            object.insert("state".into(), Value::String("evidence-unavailable".into()));
            object.insert(
                "failure".into(),
                missing_evidence(&job_id, "artifact_error=null"),
            );
        } else if state == "failed" {
            if let Some(preflight) = retained
                .manifest
                .as_ref()
                .and_then(|value| value.get("preflight"))
                .filter(|value| value.get("ready").and_then(Value::as_bool) == Some(false))
            {
                let missing = preflight
                    .get("missing")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| "target prerequisites".into());
                let remediation = preflight
                    .get("remediation")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .unwrap_or_default();
                let detail = format!(
                    "missing: {missing}{}",
                    if remediation.is_empty() {
                        String::new()
                    } else {
                        format!("; remediation: {remediation}")
                    }
                );
                let failure = Failure::config("stado.worker", detail);
                let object = result.as_object_mut().expect("object");
                object.insert("preflight".into(), preflight.clone());
                object.insert("failure".into(), failure_summary(&failure, format!("Job {job_id} did not execute because the selected host is missing: {missing}.")));
            }
        }
    }
    Ok(result)
}

fn require_immutable_native(
    target: &str,
    provision: Option<&Provision>,
    app_repo: Option<&Path>,
    identity: &Identity,
) -> Answer {
    if !matches!(provision, Some(Provision::NativeBinary { .. })) {
        return Ok(());
    }
    if target != "tui" {
        return Err(Failure::config(
            "stado.submit",
            "--app-binary-path is supported only for remote TUI runs and authoring.",
        ));
    }
    if app_repo.is_none() {
        return Err(Failure::config(
            "stado.pack",
            "Remote native-binary provisioning needs --app-repo <path>.",
        ));
    }
    let primary = identity
        .document
        .pointer("/app/repositories")
        .and_then(Value::as_array)
        .and_then(|repositories| {
            repositories
                .iter()
                .find(|value| value.get("index").and_then(Value::as_u64) == Some(0))
        });
    let clean = primary
        .and_then(|value| value.get("gitSha"))
        .and_then(Value::as_str)
        .is_some()
        && primary
            .and_then(|value| value.get("dirty"))
            .and_then(Value::as_bool)
            == Some(false);
    if !clean {
        return Err(Failure::config(
            "stado.pack",
            "--app-binary-path requires --app-repo to be a clean committed source checkout.",
        ));
    }
    Ok(())
}

fn safe_author_name(value: &str, label: &str) -> Result<String, Failure> {
    let clean = value.trim();
    let mut bytes = clean.bytes();
    let valid = matches!(bytes.next(), Some(byte) if byte.is_ascii_alphanumeric())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if !valid {
        return Err(Failure::config(
            "stado.submit",
            format!("{label} must be one safe path name: {value}"),
        ));
    }
    Ok(clean.to_string())
}

fn repository_root(
    harness: &Path,
    app_id: &str,
    explicit: Option<&Path>,
) -> Result<PathBuf, Failure> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    let application = manifest::load(harness, app_id)?;
    let root = application
        .document
        .get("repositories")
        .and_then(serde_yaml::Value::as_sequence)
        .and_then(|items| items.first())
        .and_then(|value| value.get("root"))
        .and_then(serde_yaml::Value::as_str)
        .ok_or_else(|| {
            Failure::config(
                "stado.pack",
                format!("app {app_id} has no primary repository root"),
            )
        })?;
    Ok(PathBuf::from(root))
}

fn model_router_url(value: Option<&str>) -> Result<String, Failure> {
    let clean = value.unwrap_or("").trim();
    if clean.is_empty() {
        return Err(Failure::config(
            "stado.submit",
            "STADO_MODEL_ROUTER_URL is required",
        ));
    }
    let secure = clean.starts_with("https://");
    let loopback = clean.starts_with("http://localhost")
        || clean.starts_with("http://127.")
        || clean.starts_with("http://[::1]");
    let authority = clean
        .split_once("://")
        .map(|(_, value)| value.split('/').next().unwrap_or(""))
        .unwrap_or("");
    if !secure && !loopback {
        return Err(Failure::config(
            "stado.submit",
            "STADO_MODEL_ROUTER_URL must use HTTPS or loopback HTTP",
        ));
    }
    if authority.contains('@') || clean.contains('?') || clean.contains('#') {
        return Err(Failure::config(
            "stado.submit",
            "STADO_MODEL_ROUTER_URL must not contain credentials, query parameters, or a fragment",
        ));
    }
    Ok(clean.trim_end_matches('/').to_string())
}

fn submit_remote_author(
    harness: &Path,
    app_id: &str,
    journey: &str,
    target: &str,
    description: &str,
    area: &str,
    host_name: &str,
    mut provision: Option<Provision>,
    app_repo: Option<&Path>,
    watch: bool,
) -> Result<Value, Failure> {
    let journey = safe_author_name(journey, "journey")?;
    let area = safe_author_name(area, "authoring area")?;
    if registration_directory(target).is_none() {
        return Err(Failure::config(
            "stado.submit",
            format!("Remote authoring does not support target \"{target}\"."),
        ));
    }
    let selected = host(host_name, "stado.submit")?;
    require_gui_ready(target, &selected)?;
    let application = manifest::load(harness, app_id)?;
    let configured_router = application
        .document
        .get("surfaces")
        .and_then(|value| value.get(target))
        .and_then(|value| value.get("conditions"))
        .and_then(|value| value.get("STADO_MODEL_ROUTER_URL"))
        .and_then(serde_yaml::Value::as_str)
        .map(str::to_string)
        .or_else(|| std::env::var("STADO_MODEL_ROUTER_URL").ok());
    let router = model_router_url(configured_router.as_deref())?;
    let product_root = repository_root(harness, app_id, app_repo)?;
    let identity = pack_source_identity(harness, app_id, Some(&product_root))?;
    require_immutable_native(target, provision.as_ref(), app_repo, &identity)?;
    let packed = pack_repo(harness, &[app_id])?;
    let repo_uri = upload(&packed.file, &format!("probierz-{}.tar.gz", packed.hash))?;
    let identity_uri = upload(
        &identity.file,
        &format!("source-{app_id}-{}.json", identity.hash),
    )?;
    let provisioned = provision_inputs(app_id, &mut provision, Some(&product_root), true)?;
    let receipt_id = format!("remote-{}", uuid_v4()?);
    let author = (
        journey.as_str(),
        area.as_str(),
        description,
        receipt_id.as_str(),
    );
    let environment = std::env::var("PROBIERZ_MODEL")
        .ok()
        .map(|value| vec![("PROBIERZ_MODEL".to_string(), value)])
        .unwrap_or_default();
    let script = run_script(
        target,
        app_id,
        None,
        provision.as_ref(),
        &packed.hash,
        selected.platform,
        "author",
        Some(author),
        Some(&router),
        false,
        &environment,
    )?;
    let script_file = work_path(&format!("probierz-author-{}.sh", packed.hash))?;
    fs::write(&script_file, script)?;
    let script_uri = upload(&script_file, &format!("author-{}.sh", packed.hash))?;
    let mut inputs = Map::new();
    inputs.insert(
        "repo".into(),
        json!({ "stado_uri": repo_uri, "relative_path": "inputs/probierz.tar.gz" }),
    );
    inputs.insert(
        "script".into(),
        json!({ "stado_uri": script_uri, "relative_path": "inputs/run.sh" }),
    );
    inputs.insert(
        "source".into(),
        json!({ "stado_uri": identity_uri, "relative_path": "inputs/source-identity.json" }),
    );
    inputs.extend(provisioned);
    let watch_budget = conservative_watch_budget(harness, app_id)?;
    let secrets = remote_secret_env(
        harness,
        app_id,
        &["STADO_MODEL_ROUTER_TOKEN", "PROBIERZ_MODEL_AGENT_SECRET"],
    )?;
    let submission = submit_machine(
        harness,
        &selected,
        &packed.hash,
        "author",
        inputs.clone(),
        secrets,
        watch_budget,
    )?;
    let identity_fields = copy_submission_identity(
        &identity,
        provision.as_ref(),
        &submission.receipt_dir,
        &inputs,
    )?;
    let mut result = json!({
        "host": host_name,
        "jobId": submission.job_id,
        "target": target,
        "appId": app_id,
        "journey": journey,
        "area": area,
        "submitted": submission.job_id.is_some(),
        "watchBudgetMs": submission.watch_budget_ms,
    });
    result
        .as_object_mut()
        .expect("object")
        .extend(identity_fields);
    let Some(job_id) = submission.job_id else {
        let object = result.as_object_mut().expect("object");
        object.insert("state".into(), Value::String("submit-failed".into()));
        object.insert("failure".into(), submission.failure.unwrap_or(Value::Null));
        return Ok(result);
    };
    let test_directory = application
        .document
        .get("surfaces")
        .and_then(|value| value.get(target))
        .and_then(|value| value.get("testDirectory"))
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("tests");
    let source_receipt = save_author_submission(
        harness,
        &job_id,
        app_id,
        &journey,
        &area,
        target,
        &product_root,
        test_directory,
        &identity,
    )?;
    result.as_object_mut().expect("object").insert(
        "sourceReceipt".into(),
        Value::String(source_receipt.display().to_string()),
    );
    if !watch {
        let object = result.as_object_mut().expect("object");
        object.insert("state".into(), Value::String("queued".into()));
        object.insert("failure".into(), Value::Null);
        return Ok(result);
    }
    let watched = watch_job(harness, &job_id, &selected, Some(watch_budget))?;
    result
        .as_object_mut()
        .expect("object")
        .extend(watched.as_object().cloned().unwrap_or_default());
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if matches!(state.as_str(), "completed" | "failed") {
        let retained = fetch_run_evidence(harness, &job_id, &selected)?;
        if let Some(path) = &retained.results_dir {
            result.as_object_mut().expect("object").insert(
                "resultsDir".into(),
                Value::String(path.display().to_string()),
            );
        }
        if let Some(error) = &retained.artifact_error {
            result
                .as_object_mut()
                .expect("object")
                .insert("artifactError".into(), error.clone());
        }
        if state == "completed" && retained.results_dir.is_none() {
            let object = result.as_object_mut().expect("object");
            object.insert("state".into(), Value::String("evidence-unavailable".into()));
            object.insert(
                "failure".into(),
                missing_evidence(&job_id, "artifact_error=null"),
            );
        } else if state == "completed" {
            if let Some(Value::Object(authored)) =
                restore_remote_authoring(harness, &job_id, &retained, Some(app_id), true)?
            {
                result.as_object_mut().expect("object").extend(authored);
            }
            result.as_object_mut().expect("object").insert(
                "specDir".into(),
                registration_directory(target)
                    .map(Value::from)
                    .unwrap_or(Value::Null),
            );
        }
    }
    Ok(result)
}

fn uuid_v4() -> Result<String, Failure> {
    let mut bytes = [0_u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let encoded = hex::encode(bytes);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &encoded[0..8],
        &encoded[8..12],
        &encoded[12..16],
        &encoded[16..20],
        &encoded[20..32],
    ))
}

fn submit_remote_seo(harness: &Path, app_id: &str, args: SeoArgs) -> Result<Value, Failure> {
    let selected = host(&args.host, "stado.submit")?;
    let (base_url, primary, secondary, adjudicator) = match (
        args.base_url.as_deref(), args.primary_model.as_deref(),
        args.secondary_model.as_deref(), args.adjudicator_model.as_deref(),
    ) {
        (Some(base), Some(primary), Some(secondary), Some(adjudicator)) => (base, primary, secondary, adjudicator),
        _ => return Err(Failure::config(
            "stado.submit",
            "Remote SEO evaluation needs --base-url, --primary-model, --secondary-model, and --adjudicator-model.",
        )),
    };
    let application = manifest::load(harness, app_id)?;
    let profile = application
        .document
        .get("seo")
        .and_then(|value| value.get("profiles"))
        .and_then(|value| value.get(&args.mode))
        .ok_or_else(|| {
            Failure::config(
                "stado.submit",
                format!("app {app_id} has no SEO profile for {}", args.mode),
            )
        })?;
    let signature = profile
        .get("requireSignature")
        .and_then(serde_yaml::Value::as_bool)
        .unwrap_or(false);
    let needs_production = profile
        .get("requireProductionEvidence")
        .and_then(serde_yaml::Value::as_bool)
        .unwrap_or(false);
    if needs_production && args.production_evidence.is_none() {
        return Err(Failure::config(
            "stado.submit",
            format!("{} SEO profile requires --production-evidence", args.mode),
        ));
    }
    let policy = args
        .policy
        .as_deref()
        .or_else(|| manifest_string(&application.document, &["seo", "policy"]))
        .ok_or_else(|| Failure::config("stado.submit", "seo.policy is required"))?;
    let brief = args
        .brief
        .as_deref()
        .or_else(|| manifest_string(&application.document, &["seo", "brief"]))
        .ok_or_else(|| Failure::config("stado.submit", "seo.brief is required"))?;
    if let Some(file) = args.production_evidence.as_deref() {
        if !file.exists() {
            return Err(Failure::config(
                "stado.submit",
                format!("production SEO evidence not found: {}", file.display()),
            ));
        }
    }
    let packed = pack_repo(harness, &[app_id])?;
    let repo_uri = upload(&packed.file, &format!("probierz-{}.tar.gz", packed.hash))?;
    let router = model_router_url(std::env::var("STADO_MODEL_ROUTER_URL").ok().as_deref())?;
    let script = seo_script(
        app_id,
        base_url,
        &args.mode,
        policy,
        brief,
        primary,
        secondary,
        adjudicator,
        &args.agent_id,
        &router,
        args.production_evidence.is_some(),
        signature,
        &packed.hash,
    );
    let script_file = work_path(&format!("probierz-seo-{}.sh", packed.hash))?;
    fs::write(&script_file, script)?;
    let mut inputs = Map::new();
    inputs.insert(
        "repo".into(),
        json!({ "stado_uri": repo_uri, "relative_path": "inputs/probierz.tar.gz" }),
    );
    inputs.insert(
        "script".into(),
        json!({
            "stado_uri": upload(&script_file, &format!("seo-{}.sh", packed.hash))?,
            "relative_path": "inputs/run.sh",
        }),
    );
    if let Some(file) = args.production_evidence.as_deref() {
        inputs.insert(
            "productionEvidence".into(),
            json!({
                "stado_uri": upload(file, &format!("seo-production-{}.json", packed.hash))?,
                "relative_path": "inputs/production-evidence.json",
            }),
        );
    }
    let mut secret_names = vec!["STADO_MODEL_ROUTER_TOKEN", "PROBIERZ_MODEL_AGENT_SECRET"];
    if signature {
        secret_names.push("PROBIERZ_SEO_RECEIPT_PRIVATE_KEY");
    }
    let secrets = remote_secret_env(harness, app_id, &secret_names)?;
    let watch_budget = conservative_watch_budget(harness, app_id)?;
    let submission = submit_machine(
        harness,
        &selected,
        &packed.hash,
        "seo",
        inputs,
        secrets,
        watch_budget,
    )?;
    let mut result = json!({
        "host": args.host,
        "jobId": submission.job_id,
        "appId": app_id,
        "mode": args.mode,
        "submitted": submission.job_id.is_some(),
        "watchBudgetMs": submission.watch_budget_ms,
    });
    let Some(job_id) = submission.job_id else {
        let object = result.as_object_mut().expect("object");
        object.insert("state".into(), Value::String("submit-failed".into()));
        object.insert("failure".into(), submission.failure.unwrap_or(Value::Null));
        return Ok(result);
    };
    if args.no_watch {
        let object = result.as_object_mut().expect("object");
        object.insert("state".into(), Value::String("queued".into()));
        object.insert("failure".into(), Value::Null);
        return Ok(result);
    }
    let watched = watch_job(harness, &job_id, &selected, Some(watch_budget))?;
    result
        .as_object_mut()
        .expect("object")
        .extend(watched.as_object().cloned().unwrap_or_default());
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if matches!(state.as_str(), "completed" | "failed") {
        let retained = fetch_run_evidence(harness, &job_id, &selected)?;
        if let Some(path) = retained.results_dir {
            result.as_object_mut().expect("object").insert(
                "resultsDir".into(),
                Value::String(path.display().to_string()),
            );
        }
        if let Some(error) = retained.artifact_error {
            result
                .as_object_mut()
                .expect("object")
                .insert("artifactError".into(), error);
        }
        if state == "completed" && result.get("resultsDir").is_none() {
            let object = result.as_object_mut().expect("object");
            object.insert("state".into(), Value::String("evidence-unavailable".into()));
            object.insert(
                "failure".into(),
                missing_evidence(&job_id, "artifact_error=null"),
            );
        }
    }
    Ok(result)
}

/// Inputs for the dedicated Byk iOS host bridge. The OTP broker stays local;
/// Stado carries an authenticated loopback bridge to the worker's protected socket.
#[derive(Debug)]
pub struct RemoteBykRequest<'a> {
    pub host_selector: &'a str,
    pub root: &'a Path,
    pub app_path: &'a Path,
    pub ios_device: &'a str,
    pub ios_version: &'a str,
    pub socket_path: &'a Path,
    pub recipient: &'a str,
}

#[derive(Debug)]
pub struct RemoteBykOutcome {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

const BYK_RETRIES: usize = 3;
const BYK_QUARANTINE: Duration = Duration::from_secs(15 * 60);

pub fn source_file_list(root: &Path) -> Result<Vec<u8>, Failure> {
    let output = sh(
        "git",
        &[
            "-C".into(),
            root.display().to_string(),
            "ls-files".into(),
            "--cached".into(),
            "--others".into(),
            "--exclude-standard".into(),
            "-z".into(),
        ],
        None,
        None,
        None,
    );
    if output.status != Some(0) {
        return Err(Failure::config(
            "run.source",
            format!(
                "git ls-files in {}: {}",
                root.display(),
                process_text(&output)
            ),
        ));
    }
    let mut files: Vec<String> = output
        .stdout
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .filter(|relative| {
            let path = Path::new(relative);
            let secret = path
                .components()
                .any(|part| part.as_os_str().to_string_lossy().starts_with(".env"))
                || path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("probierz-") && name.ends_with(".json"));
            let excluded = path.components().any(|part| {
                matches!(
                    part.as_os_str().to_str(),
                    Some("node_modules" | "test-results" | "..")
                )
            });
            !secret
                && !excluded
                && fs::symlink_metadata(root.join(path))
                    .map(|metadata| metadata.is_file() || metadata.file_type().is_symlink())
                    .unwrap_or(false)
        })
        .map(str::to_string)
        .collect();
    if root.join("package-lock.json").exists()
        && !files.iter().any(|path| path == "package-lock.json")
    {
        files.push("package-lock.json".into());
    }
    files.sort();
    let mut answer = Vec::new();
    for file in files {
        answer.extend(file.as_bytes());
        answer.push(0);
    }
    Ok(answer)
}

pub fn run_remote_byk_auth(request: RemoteBykRequest<'_>) -> Result<RemoteBykOutcome, Failure> {
    RECEIVED_SIGNAL.store(0, Ordering::SeqCst);
    install_byk_signal_handlers();
    require_local_kind(request.root, true, "Probierz root")?;
    require_local_kind(request.app_path, true, "APP_IOS")?;
    if !fs::metadata(request.socket_path)?.file_type().is_socket() {
        return Err(Failure::config(
            "byk.remote",
            "local OTP broker socket is unavailable",
        ));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Failure::config("byk.remote", "HOME is required"))?;
    assert_byk_host_available(&home)?;
    let target = resolve_byk_target(request.host_selector)?;
    retry_byk("Stado reachability check", || {
        sh_with_input(
            STADO_BIN,
            &[
                "host".into(),
                "ping".into(),
                target.registry_target.clone(),
                "--json".into(),
            ],
            &[],
        )
    })?;
    let source_files = source_file_list(request.root)?;
    if source_files.is_empty() {
        return Err(Failure::config(
            "byk.remote",
            "Probierz source set is empty",
        ));
    }
    let run_id = uuid_v4()?;
    let remote_run_relative = format!(".stado/work/runs/{run_id}");
    let run_root = target
        .remote_home
        .join(".stado")
        .join("work")
        .join("runs")
        .join(&run_id);
    let remote_source = run_root.join("probierz");
    let remote_app = run_root.join("Byk.app");
    let remote_socket = run_root.join("byk-otp.sock");
    let worker = remote_source.join("probierz-rs/target/release/probierz");
    let remote_port = byk_forward_port(&run_id);
    let bridge_token = uuid_v4()?;
    let bridge = BykLocalBridge::start(request.socket_path, &bridge_token)?;
    let forward_name = format!("probierz-byk-{}", run_id.replace('-', ""));
    let forward_args = vec![
        "host".into(),
        "forward-local".into(),
        target.registry_target.clone(),
        forward_name.clone(),
        "--remote-port".into(),
        remote_port.to_string(),
        "--local-port".into(),
        bridge.port().to_string(),
        "--json".into(),
    ];
    let mut forward_opened = false;
    let mut created = false;
    let result = (|| {
        retry_byk("Stado OTP forwarding channel", || {
            sh_with_input(STADO_BIN, &forward_args, &[])
        })?;
        forward_opened = true;
        retry_byk("dedicated-host preparation", || {
            sh_with_input(
                STADO_BIN,
                &[
                    "host".into(),
                    "exec".into(),
                    target.registry_target.clone(),
                    "--".into(),
                    "mkdir".into(),
                    "-p".into(),
                    ".stado/work/runs".into(),
                ],
                &[],
            )
        })?;
        created = true;
        retry_byk("Probierz source delivery", || {
            sh_with_input(
                STADO_BIN,
                &[
                    "host".into(),
                    "deliver".into(),
                    target.registry_target.clone(),
                    request.root.display().to_string(),
                    format!("{remote_run_relative}/probierz"),
                    "--files-from".into(),
                    "-".into(),
                    "--json".into(),
                ],
                &source_files,
            )
        })?;
        retry_byk("Byk app delivery", || {
            sh_with_input(
                STADO_BIN,
                &[
                    "host".into(),
                    "deliver".into(),
                    target.registry_target.clone(),
                    request.app_path.display().to_string(),
                    format!("{remote_run_relative}/Byk.app"),
                    "--json".into(),
                ],
                &[],
            )
        })?;
        retry_byk("dedicated-host worker build", || {
            sh_with_input(
                STADO_BIN,
                &[
                    "host".into(),
                    "build".into(),
                    target.registry_target.clone(),
                    "--manifest-path".into(),
                    remote_source
                        .join("probierz-rs/Cargo.toml")
                        .display()
                        .to_string(),
                    "--bin".into(),
                    "probierz".into(),
                    "--json".into(),
                ],
                &[],
            )
        })?;
        let config = json!({
            "runRoot": run_root,
            "sourceRoot": remote_source,
            "appPath": remote_app,
            "socketPath": remote_socket,
            "otpPort": remote_port,
            "bridgeToken": bridge_token,
            "recipient": request.recipient,
            "iosDevice": request.ios_device,
            "iosVersion": request.ios_version,
        });
        let mut input = serde_json::to_vec(&config)?;
        input.push(b'\n');
        let output = sh_with_input(
            STADO_BIN,
            &[
                "host".into(),
                "run-attached".into(),
                target.registry_target.clone(),
                "--program".into(),
                worker.display().to_string(),
                "--arg".into(),
                "stado".into(),
                "--arg".into(),
                "byk-auth-worker".into(),
            ],
            &input,
        );
        if output.status == Some(255) {
            return Err(Failure::unavailable(
                "byk.remote",
                "dedicated-host worker transport failed",
            ));
        }
        let received = RECEIVED_SIGNAL.load(Ordering::SeqCst);
        Ok(RemoteBykOutcome {
            code: if received == 0 { output.status } else { None },
            signal: if received == 0 {
                output.signal
            } else {
                Some(received)
            },
        })
    })();
    if created {
        let _ = sh_with_input(
            STADO_BIN,
            &[
                "host".into(),
                "remove-run-directory".into(),
                target.registry_target.clone(),
                run_root.display().to_string(),
                "--json".into(),
            ],
            &[],
        );
    }
    if forward_opened {
        let _ = sh_with_input(
            STADO_BIN,
            &[
                "host".into(),
                "forward-close".into(),
                target.registry_target.clone(),
                forward_name,
                "--json".into(),
            ],
            &[],
        );
    }
    drop(bridge);
    match &result {
        Ok(_) => clear_byk_quarantine(&home)?,
        Err(failure) if failure.code == Code::Unavailable => {
            quarantine_byk_host(&home, request.host_selector, &failure.detail)?
        }
        Err(_) => {}
    }
    result
}

#[derive(Debug)]
struct BykTarget {
    registry_target: String,
    remote_home: PathBuf,
}

fn resolve_byk_target(selector: &str) -> Result<BykTarget, Failure> {
    if selector.trim() != selector || !selector.starts_with("stado:") {
        return Err(Failure::config(
            "byk.remote",
            format!(
                "Stado could not resolve Byk host selector {selector:?}: expected a stado:<target> selector"
            ),
        ));
    }
    let registry_target = discovery::stado_host(selector)
        .and_then(|selected| selected.target.map(str::to_string))
        .or_else(|| {
            selector
                .strip_prefix("stado:")
                .filter(|target| !target.is_empty())
                .map(str::to_string)
        })
        .ok_or_else(|| {
            Failure::config(
                "byk.remote",
                format!(
                    "Stado could not resolve Byk host selector {selector:?}: selector has no registry target"
                ),
            )
        })?;
    let inventory = sh_with_input(
        STADO_BIN,
        &[
            "host".into(),
            "inventory".into(),
            registry_target.clone(),
            "--json".into(),
        ],
        &[],
    );
    if inventory.status != Some(0) {
        return Err(byk_resolution_failure(selector, &inventory));
    }
    let inventory: Value = serde_json::from_str(&inventory.stdout).map_err(|error| {
        Failure::config(
            "byk.remote",
            format!(
                "Stado could not resolve Byk host selector {selector:?}: invalid host inventory ({error})"
            ),
        )
    })?;
    if inventory.get("target").and_then(Value::as_str) != Some(registry_target.as_str()) {
        return Err(Failure::config(
            "byk.remote",
            format!(
                "Stado could not resolve Byk host selector {selector:?}: inventory named a different target"
            ),
        ));
    }
    if !inventory
        .get("declared_release_platform")
        .and_then(Value::as_str)
        .is_some_and(|platform| platform.starts_with("darwin-"))
    {
        return Err(Failure::config(
            "byk.remote",
            format!(
                "Stado could not place Byk host selector {selector:?}: target {registry_target:?} does not declare macOS"
            ),
        ));
    }
    let config = sh_with_input(
        STADO_BIN,
        &["host".into(), "config-show".into(), registry_target.clone()],
        &[],
    );
    if config.status != Some(0) {
        return Err(byk_resolution_failure(selector, &config));
    }
    let config: Value = serde_json::from_str(&config.stdout).map_err(|error| {
        Failure::config(
            "byk.remote",
            format!(
                "Stado could not resolve Byk host selector {selector:?}: invalid effective configuration ({error})"
            ),
        )
    })?;
    let config_file = config
        .get("file")
        .and_then(Value::as_str)
        .map(Path::new)
        .ok_or_else(|| {
            Failure::config(
                "byk.remote",
                format!(
                    "Stado could not resolve Byk host selector {selector:?}: effective configuration did not report its file"
                ),
            )
        })?;
    let remote_home = stado_home_from_config(config_file).ok_or_else(|| {
        Failure::config(
            "byk.remote",
            format!(
                "Stado could not resolve Byk host selector {selector:?}: effective configuration reported an invalid home"
            ),
        )
    })?;
    Ok(BykTarget {
        registry_target,
        remote_home,
    })
}

fn byk_resolution_failure(selector: &str, output: &ProcessOutput) -> Failure {
    let detail = process_text(output);
    Failure::config(
        "byk.remote",
        format!(
            "Stado could not resolve Byk host selector {selector:?}: {}",
            if detail.is_empty() {
                "Stado returned no diagnostic"
            } else {
                detail.as_str()
            }
        ),
    )
}

fn stado_home_from_config(file: &Path) -> Option<PathBuf> {
    if !file.is_absolute() || file.file_name()?.to_str()? != "config.json" {
        return None;
    }
    let stado = file.parent()?;
    let config = stado.parent()?;
    if stado.file_name()?.to_str()? != "stado" || config.file_name()?.to_str()? != ".config" {
        return None;
    }
    config.parent().map(Path::to_path_buf)
}

fn byk_forward_port(run_id: &str) -> u16 {
    let digest = Sha256::digest(run_id.as_bytes());
    20_000 + (u16::from_be_bytes([digest[0], digest[1]]) % 20_000)
}

fn valid_bridge_token(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

struct BykLocalBridge {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl BykLocalBridge {
    fn start(socket_path: &Path, bridge_token: &str) -> Result<Self, Failure> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
            Failure::unavailable(
                "byk.remote",
                format!("could not bind the local Stado OTP bridge: {error}"),
            )
        })?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let socket_path = socket_path.to_path_buf();
        let mut expected = bridge_token.as_bytes().to_vec();
        expected.push(b'\n');
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut tcp, _)) => {
                        let socket_path = socket_path.clone();
                        let expected = expected.clone();
                        thread::spawn(move || {
                            let _ = tcp.set_read_timeout(Some(Duration::from_secs(5)));
                            let mut received = vec![0_u8; expected.len()];
                            if tcp.read_exact(&mut received).is_ok()
                                && bool::from(received.ct_eq(&expected))
                            {
                                if let Ok(unix) = UnixStream::connect(socket_path) {
                                    relay_tcp_and_unix(tcp, unix);
                                }
                            }
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            port,
            stop,
            thread: Some(thread),
        })
    }

    fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for BykLocalBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn relay_tcp_and_unix(mut tcp: TcpStream, mut unix: UnixStream) {
    let Ok(mut tcp_reader) = tcp.try_clone() else {
        return;
    };
    let Ok(mut unix_writer) = unix.try_clone() else {
        return;
    };
    let upstream = thread::spawn(move || {
        let _ = std::io::copy(&mut tcp_reader, &mut unix_writer);
        let _ = unix_writer.shutdown(Shutdown::Write);
    });
    let _ = std::io::copy(&mut unix, &mut tcp);
    let _ = tcp.shutdown(Shutdown::Write);
    let _ = upstream.join();
}
fn require_local_kind(path: &Path, directory: bool, name: &str) -> Answer {
    if !path.is_absolute() || !path.exists() {
        return Err(Failure::config(
            "byk.remote",
            format!("{name} must be an existing absolute path"),
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(Failure::config(
            "byk.remote",
            format!("{name} has the wrong file type"),
        ));
    }
    Ok(())
}

fn byk_state_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Caches")
        .join("probierz")
        .join("remote-hosts")
        .join("byk-auth.json")
}

fn assert_byk_host_available(home: &Path) -> Answer {
    let Some(state) = read_json(&byk_state_path(home)) else {
        return Ok(());
    };
    let Some(until) = state
        .get("quarantinedUntil")
        .and_then(Value::as_str)
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
    else {
        return Ok(());
    };
    if until.with_timezone(&Utc) > Utc::now() {
        return Err(Failure::unavailable(
            "byk.remote",
            format!("dedicated host is quarantined until {}", until.to_rfc3339()),
        ));
    }
    Ok(())
}

fn quarantine_byk_host(home: &Path, selector: &str, reason: &str) -> Answer {
    let file = byk_state_path(home);
    let previous = read_json(&file);
    let now = Utc::now();
    let value = json!({
        "schemaVersion": 1,
        "host": selector,
        "failures": previous.as_ref().and_then(|value| value.get("failures")).and_then(Value::as_u64).unwrap_or(0) + 1,
        "reason": reason,
        "quarantinedAt": now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "quarantinedUntil": (now + chrono::Duration::from_std(BYK_QUARANTINE).unwrap_or_default())
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    });
    let parent = file.parent().expect("state parent");
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    let temporary = file.with_extension(format!("{}.{}.tmp", std::process::id(), nonce("byk")));
    write_json(&temporary, &value, true, true)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    fs::rename(temporary, file)?;
    Ok(())
}

fn clear_byk_quarantine(home: &Path) -> Answer {
    let file = byk_state_path(home);
    if file.exists() {
        fs::remove_file(file)?;
    }
    Ok(())
}

fn retry_byk<F>(label: &str, mut operation: F) -> Answer
where
    F: FnMut() -> ProcessOutput,
{
    let mut last = None;
    for attempt in 0..BYK_RETRIES {
        let output = operation();
        if output.status == Some(0) {
            return Ok(());
        }
        last = Some(output);
        if attempt + 1 < BYK_RETRIES {
            thread::sleep(Duration::from_secs(1_u64 << attempt));
        }
    }
    let detail = last
        .as_ref()
        .map(process_text)
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "exit unknown".into());
    Err(Failure::unavailable(
        "byk.remote",
        format!("{label} failed after {BYK_RETRIES} attempts ({detail})"),
    ))
}

fn sh_with_input(command: &str, args: &[String], input: &[u8]) -> ProcessOutput {
    let mut child = match Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return ProcessOutput {
                command: command.into(),
                args: args.to_vec(),
                status: None,
                signal: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(error.to_string()),
            }
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(input) {
            let _ = child.kill();
            return ProcessOutput {
                command: command.into(),
                args: args.to_vec(),
                status: None,
                signal: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(error.to_string()),
            };
        }
    }
    ACTIVE_CHILD.store(child.id() as i32, Ordering::SeqCst);
    let result = child.wait_with_output();
    ACTIVE_CHILD.store(0, Ordering::SeqCst);
    match result {
        Ok(output) => ProcessOutput {
            command: command.into(),
            args: args.to_vec(),
            status: output.status.code(),
            signal: output.status.signal(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            error: None,
        },
        Err(error) => ProcessOutput {
            command: command.into(),
            args: args.to_vec(),
            status: None,
            signal: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(error.to_string()),
        },
    }
}

struct BykRemoteBridge {
    socket_path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl BykRemoteBridge {
    fn start(socket_path: &Path, port: u16, bridge_token: &str) -> Result<Self, Failure> {
        if port < 1024 || socket_path.exists() || !valid_bridge_token(bridge_token) {
            return Err(Failure::config(
                "byk.worker",
                "remote Byk OTP bridge configuration is invalid",
            ));
        }
        let listener = UnixListener::bind(socket_path).map_err(|error| {
            Failure::unavailable(
                "byk.worker",
                format!("could not bind the protected OTP socket: {error}"),
            )
        })?;
        fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let mut authentication = bridge_token.as_bytes().to_vec();
        authentication.push(b'\n');
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((unix, _)) => {
                        let authentication = authentication.clone();
                        thread::spawn(move || {
                            if let Ok(mut tcp) = TcpStream::connect(("127.0.0.1", port)) {
                                if tcp.write_all(&authentication).is_ok() {
                                    relay_tcp_and_unix(tcp, unix);
                                }
                            }
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            socket_path: socket_path.to_path_buf(),
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for BykRemoteBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.socket_path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BykWorkerConfig {
    run_root: String,
    source_root: String,
    app_path: String,
    socket_path: String,
    bridge_token: String,
    recipient: String,
    ios_device: String,
    otp_port: u16,
    ios_version: String,
}

static ACTIVE_CHILD: AtomicI32 = AtomicI32::new(0);
static RECEIVED_SIGNAL: AtomicI32 = AtomicI32::new(0);

unsafe extern "C" {
    fn signal(number: i32, handler: extern "C" fn(i32)) -> usize;
    fn kill(pid: i32, signal: i32) -> i32;
}

extern "C" fn byk_signal(number: i32) {
    let previous = RECEIVED_SIGNAL.swap(number, Ordering::SeqCst);
    let pid = ACTIVE_CHILD.load(Ordering::SeqCst);
    if pid > 0 {
        // SAFETY: kill is async-signal-safe and the PID is the currently
        // running direct child published by worker_status.
        unsafe {
            let _ = kill(pid, if previous == 0 { number } else { 9 });
        }
    }
}

fn install_byk_signal_handlers() {
    // SAFETY: the handler only touches atomics and calls async-signal-safe kill.
    unsafe {
        let _ = signal(2, byk_signal);
        let _ = signal(15, byk_signal);
        let _ = signal(1, byk_signal);
    }
}

fn byk_auth_worker() -> Answer {
    let code = match byk_auth_worker_inner() {
        Ok(code) => code,
        Err(failure) => {
            eprintln!("remote Byk runner: {}", failure.detail);
            1
        }
    };
    std::process::exit(code)
}

fn byk_auth_worker_inner() -> Result<i32, Failure> {
    install_byk_signal_handlers();
    let mut input = Vec::new();
    std::io::stdin().take(4097).read_to_end(&mut input)?;
    if input.len() > 4096 {
        return Err(Failure::config(
            "byk.worker",
            "remote Byk configuration is too large",
        ));
    }
    if input.last() != Some(&b'\n') || input[..input.len().saturating_sub(1)].contains(&b'\n') {
        return Err(Failure::config(
            "byk.worker",
            "remote Byk configuration must be one JSON line",
        ));
    }
    let config: BykWorkerConfig = serde_json::from_slice(&input)
        .map_err(|_| Failure::config("byk.worker", "remote Byk configuration is invalid JSON"))?;
    if [
        config.run_root.as_str(),
        config.source_root.as_str(),
        config.app_path.as_str(),
        config.socket_path.as_str(),
        config.bridge_token.as_str(),
        config.recipient.as_str(),
        config.ios_device.as_str(),
    ]
    .iter()
    .any(|value| value.is_empty())
    {
        return Err(Failure::config(
            "byk.worker",
            "remote Byk configuration has invalid schema",
        ));
    }
    let run_root = PathBuf::from(&config.run_root);
    let source_root = protected_byk_child(&run_root, &config.source_root, "sourceRoot")?;
    let app_path = protected_byk_child(&run_root, &config.app_path, "appPath")?;
    let socket_path = protected_byk_child(&run_root, &config.socket_path, "socketPath")?;
    if !valid_email(&config.recipient) {
        return Err(Failure::config(
            "byk.worker",
            "remote Byk recipient is invalid",
        ));
    }
    if config.ios_device.trim() != config.ios_device || config.ios_device.contains(['\r', '\n']) {
        return Err(Failure::config(
            "byk.worker",
            "remote iOS device name is invalid",
        ));
    }
    if !config.ios_version.is_empty()
        && !config
            .ios_version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(Failure::config(
            "byk.worker",
            "remote iOS version is invalid",
        ));
    }
    if !app_path.is_dir() {
        return Err(Failure::config(
            "byk.worker",
            "remote Byk app is unavailable",
        ));
    }
    let otp_bridge = BykRemoteBridge::start(&socket_path, config.otp_port, &config.bridge_token)?;
    let lock_path = run_root
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| Failure::config("byk.worker", "remote Byk run root is invalid"))?
        .join("byk-auth.lock");
    let npm_cache = run_root.join("npm-cache");
    let appium_home = run_root.join("appium-home");
    let modules = appium_home.join("node_modules");
    let driver = source_root
        .join("node_modules")
        .join("appium-xcuitest-driver");
    if fs::create_dir(&lock_path).is_err() {
        return Err(Failure::config(
            "byk.worker",
            "dedicated iOS host is already running a Byk device test",
        ));
    }
    fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o700))?;
    let result = (|| {
        let sdk = worker_status(
            "/usr/bin/xcrun",
            &["--sdk", "iphonesimulator", "--show-sdk-version"],
            &source_root,
            &base_byk_environment(),
        )?;
        if sdk != 0 {
            return Err(Failure::config(
                "byk.worker",
                "dedicated iOS host is missing the Xcode iOS Simulator SDK",
            ));
        }
        let mut install_env = base_byk_environment();
        install_env.insert("NPM_CONFIG_CACHE".into(), npm_cache.display().to_string());
        let install = worker_status(
            "/opt/homebrew/bin/npm",
            &[
                "ci",
                "--workspace",
                "packages/mobile",
                "--include-workspace-root=false",
            ],
            &source_root,
            &install_env,
        )?;
        if install != 0 {
            return Ok(install);
        }
        fs::create_dir_all(&modules)?;
        std::os::unix::fs::symlink(&driver, modules.join("appium-xcuitest-driver"))?;
        if npm_cache.exists() {
            fs::remove_dir_all(&npm_cache)?;
        }
        if RECEIVED_SIGNAL.load(Ordering::SeqCst) != 0 {
            return Ok(1);
        }
        let mut environment = base_byk_environment();
        environment.insert("PROBIERZ_SPEC".into(), "byk-auth.e2e.ts".into());
        environment.insert("BYK_OTP_SOCKET".into(), socket_path.display().to_string());
        environment.insert("BYK_TEST_EMAIL".into(), config.recipient.clone());
        environment.insert("APP_IOS".into(), app_path.display().to_string());
        environment.insert("APPIUM_HOME".into(), appium_home.display().to_string());
        environment.insert("IOS_DEVICE".into(), config.ios_device.clone());
        if !config.ios_version.is_empty() {
            environment.insert("IOS_VERSION".into(), config.ios_version.clone());
        }
        worker_status(
            "/opt/homebrew/bin/npm",
            &["run", "test:mobile:ios"],
            &source_root,
            &environment,
        )
    })();
    let _ = fs::remove_dir_all(&lock_path);
    drop(otp_bridge);
    let _ = fs::remove_dir_all(&run_root);
    result
}

fn protected_byk_child(root: &Path, candidate: &str, name: &str) -> Result<PathBuf, Failure> {
    let candidate = PathBuf::from(candidate);
    if !candidate.is_absolute() || !candidate.starts_with(root) || candidate == root {
        return Err(Failure::config(
            "byk.worker",
            format!("{name} must stay inside the protected run directory"),
        ));
    }
    Ok(candidate)
}

fn valid_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !value.chars().any(char::is_whitespace)
}

fn base_byk_environment() -> BTreeMap<String, String> {
    const NAMES: &[&str] = &[
        "PATH",
        "HOME",
        "TMPDIR",
        "USER",
        "SHELL",
        "LANG",
        "TERM",
        "COLORTERM",
        "FORCE_COLOR",
        "NO_COLOR",
        "CLICOLOR",
        "CLICOLOR_FORCE",
        "APPIUM_HOME",
        "DEVELOPER_DIR",
        "SDKROOT",
        "TOOLCHAINS",
        "XCODE_DEFAULT_TOOLCHAIN_OVERRIDE",
        "XCODE_DEVELOPER_USR_PATH",
        "XCODE_PRODUCT_BUILD_VERSION",
        "XCODE_TOOLCHAIN_PATH",
        "XCODE_VERSION_ACTUAL",
        "XCODE_VERSION_MAJOR",
        "XCODE_VERSION_MINOR",
        "XCODE_XCCONFIG_FILE",
        "IOS_DEVICE",
        "IOS_VERSION",
        "CI",
    ];
    let mut environment = BTreeMap::new();
    for (name, value) in std::env::vars() {
        if NAMES.contains(&name.as_str()) || name.starts_with("LC_") {
            environment.insert(name, value);
        }
    }
    let system = "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin";
    let path = environment
        .get("PATH")
        .map(|value| format!("{system}:{value}"))
        .unwrap_or_else(|| system.into());
    environment.insert("PATH".into(), path);
    environment
}

fn worker_status(
    command: &str,
    args: &[&str],
    cwd: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<i32, Failure> {
    let mut child = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| Failure::config("byk.worker", format!("could not start {command}")))?;
    ACTIVE_CHILD.store(child.id() as i32, Ordering::SeqCst);
    let status = child.wait()?;
    ACTIVE_CHILD.store(0, Ordering::SeqCst);
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(status: i32) -> ProcessOutput {
        ProcessOutput {
            command: "stado".into(),
            args: Vec::new(),
            status: Some(status),
            signal: None,
            stdout: String::new(),
            stderr: String::new(),
            error: None,
        }
    }

    #[test]
    fn machine_request_has_the_stado_protocol_shape_and_only_secret_coordinates() {
        let selected = discovery::stado_host("stado:gcp").expect("host");
        let mut inputs = Map::new();
        inputs.insert(
            "repo".into(),
            json!({
                "stado_uri": "stado://probierz/inputs/probierz-fixed.tar.gz",
                "relative_path": "inputs/probierz.tar.gz",
            }),
        );
        let mut request = Map::new();
        request.insert(
            "client_request_id".into(),
            Value::String("probierz-run-fixed".into()),
        );
        request.insert(
            "command".into(),
            Value::String("PROBIERZ_WATCH_BUDGET_MS=1234 bash inputs/run.sh".into()),
        );
        request.insert(
            "output_uri".into(),
            Value::String("stado://probierz/results".into()),
        );
        request.insert("input_objects".into(), Value::Object(inputs));
        request.insert(
            "secret_env".into(),
            json!({
                "STADO_MODEL_ROUTER_TOKEN": { "item": "probierz-model-router", "field": "token" },
            }),
        );
        for (name, value) in selected
            .request
            .expect("request")
            .as_object()
            .expect("object")
        {
            request.insert(name.clone(), value.clone());
        }
        assert_eq!(
            serde_json::to_string(&Value::Object(request)).expect("json"),
            r#"{"client_request_id":"probierz-run-fixed","command":"PROBIERZ_WATCH_BUDGET_MS=1234 bash inputs/run.sh","output_uri":"stado://probierz/results","input_objects":{"repo":{"stado_uri":"stado://probierz/inputs/probierz-fixed.tar.gz","relative_path":"inputs/probierz.tar.gz"}},"secret_env":{"STADO_MODEL_ROUTER_TOKEN":{"item":"probierz-model-router","field":"token"}},"provider":"gcp","pin_to_provider":true}"#,
        );
    }

    #[test]
    fn upload_retries_six_times_with_five_seconds_more_each_time() {
        let mut calls = 0;
        let mut delays = Vec::new();
        let error = upload_with(
            Path::new("/tmp/input"),
            "input.tar.gz",
            |_, _| {
                calls += 1;
                process(STADO_RETRY_EXIT)
            },
            |delay| delays.push(delay),
        )
        .expect_err("upload must fail");
        assert_eq!(calls, 6);
        assert_eq!(
            delays,
            vec![
                Duration::from_secs(5),
                Duration::from_secs(10),
                Duration::from_secs(15),
                Duration::from_secs(20),
                Duration::from_secs(25),
            ]
        );
        assert_eq!(error.point, "stado.upload");
    }

    #[test]
    fn environment_names_are_shell_identifiers_and_values_may_contain_equals() {
        assert_eq!(
            parse_environment(&["GOOD_name=a=b".into()]).expect("valid"),
            vec![("GOOD_name".into(), "a=b".into())],
        );
        assert_eq!(
            parse_environment(&["9bad=value".into()])
                .expect_err("invalid")
                .detail,
            "--env needs NAME=VALUE with a valid environment variable name",
        );
    }

    #[test]
    fn job_identity_contracts_are_not_path_names() {
        assert!(canonical_job_id("job-0123456789abcdef01234567"));
        assert!(!canonical_job_id("job-0123"));
        assert!(safe_job_identifier("job-legacy_1"));
        assert!(!safe_job_identifier("../job"));
    }
    #[test]
    fn retained_paths_and_byk_worker_paths_refuse_parent_escape() {
        let root = Path::new("/tmp/probierz-protected");
        assert!(safe_child(root, "../../escape", "unsafe").is_err());
        assert!(protected_byk_child(root, "/tmp/escape", "work directory").is_err());
        assert_eq!(
            protected_byk_child(root, "/tmp/probierz-protected/work", "work directory")
                .expect("protected child"),
            root.join("work"),
        );
    }

    #[test]
    fn byk_email_validation_requires_a_nonempty_dotted_domain() {
        assert!(valid_email("operator@example.com"));
        assert!(!valid_email("operator@example"));
        assert!(!valid_email("@example.com"));
        assert!(!valid_email("operator @example.com"));
    }

    #[test]
    fn remote_worker_bootstraps_the_locked_rust_and_node_products() {
        let script = run_script(
            "tui",
            "stado",
            None,
            None,
            "fixed",
            Some("linux"),
            "run",
            None,
            None,
            false,
            &[],
        )
        .expect("remote script");
        assert!(script.contains("cargo build --locked --release"));
        assert!(script.contains("node-v22.20.0-linux-x64.tar.xz"));
        assert!(script.contains("npm ci --no-audit --no-fund --loglevel=error"));
        assert!(script.contains("\"$PROBIERZ\" --harness \"$HARNESS\" run tui"));
        assert!(!script.contains("node agent/"));
    }

    #[test]
    fn source_snapshot_contains_product_manifests_but_not_runtime_results() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repository root");
        let list = source_file_list(root).expect("source list");
        let files: Vec<_> = list
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
            .map(|entry| String::from_utf8_lossy(entry).into_owned())
            .collect();
        assert!(files.iter().any(|entry| entry == "package.json"));
        assert!(files.iter().any(|entry| entry == "probierz-rs/Cargo.toml"));
        assert!(!files.iter().any(|entry| entry.starts_with("test-results/")));
    }
}
