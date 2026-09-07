//! Probierz stdio MCP server.
//!
//! Requests are newline-delimited JSON-RPC. Discovery is served without side
//! effects; mutating tools run only after an explicit `tools/call` request.

#[allow(dead_code)]
#[path = "failure.rs"]
mod failure;

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand_core::{OsRng, RngCore};
use serde_json::{json, Map, Value};

use failure::now_iso;

const PROTOCOL_VERSION: &str = "2024-11-05";
const TOOLS_JSON: &str = r###"[{"name":"probierz_list_surfaces","description":"List the cross-platform test surfaces (web, electron, mobile, desktop-native): tool, npm script, targets, and relevant env vars.","inputSchema":{"type":"object","properties":{},"required":[]}},{"name":"probierz_list_specs","description":"Discover e2e/spec files on disk; optional surface narrows to one (web|electron|mobile|desktop-native).","inputSchema":{"type":"object","properties":{"surface":{"type":"string","description":"Optional surface filter."}},"required":[]}},{"name":"probierz_describe_spec","description":"Static outline of a spec (describe/it/test titles in file order) by its path under the probierz root. Does not execute anything.","inputSchema":{"type":"object","properties":{"spec":{"type":"string","description":"Spec path, e.g. packages/mobile/test/specs/byk.e2e.ts"}},"required":["spec"]}},{"name":"probierz_run_command","description":"Return the exact shell command to run a target yourself (web|electron|mobile:ios|mobile:android|desktop:mac|desktop:win). Read-only: probierz never runs it.","inputSchema":{"type":"object","properties":{"target":{"type":"string","description":"One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win."}},"required":["target"]}},{"name":"probierz_check","description":"Preflight a target's toolchain WITHOUT running anything: reports whether it is ready and, for each missing piece, exactly how to fix it -- `probierz setup <target>` for parts probierz owns (Playwright browsers, Appium drivers) or a host install command for the rest (Xcode, Android SDK, simulators, WinAppDriver). Read-only.","inputSchema":{"type":"object","properties":{"target":{"type":"string","description":"One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win."}},"required":["target"]}},{"name":"probierz_setup","description":"Install the toolchain parts probierz owns for a target (npm deps + Playwright browsers, or npm deps + the Appium driver). Does NOT install host-level dependencies (Xcode, Android SDK, simulators, WinAppDriver) -- probierz_check reports those. Side-effecting: runs npm / appium driver install.","inputSchema":{"type":"object","properties":{"target":{"type":"string","description":"One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win."},"timeoutMs":{"type":"number","description":"Kill a setup step after this many ms (default 30 min)."}},"required":["target"]}},{"name":"probierz_run","description":"EXECUTE a target end-to-end, capture evidence, analyze it, and dispatch a bounded Brama repair worker when it fails. Heavy + side-effecting: needs the target toolchain; noRepair=true records without repair.","inputSchema":{"type":"object","properties":{"target":{"type":"string","description":"One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win."},"record":{"type":"boolean","description":"Force video + trace + screenshot capture on."},"appId":{"type":"string","description":"Product identifier used in the run-scoped artifact path and manifest."},"env":{"type":"object","description":"Condition env vars, e.g. { BASE_URL, APP_IOS, PROBIERZ_LOCALE, PROBIERZ_COLOR_SCHEME }."},"timeoutMs":{"type":"number","description":"Kill the run after this many ms (default 20 min)."},"resourceWaitMs":{"type":"number","description":"Wait this long for a busy device/port lease; 0 fails fast."},"frames":{"type":"number","description":"Extract this many frames per recorded video (needs ffmpeg)."},"analyze":{"type":"boolean","description":"Analyze the report after the run (default true)."},"force":{"type":"boolean","description":"Skip the preflight gate and spawn even if the toolchain looks incomplete."},"spec":{"type":"string","description":"Run only this one spec (path/substring), e.g. packages/mobile/test/specs/byk.e2e.ts, to scope the run to a single app's suite."},"noRepair":{"type":"boolean","description":"Record a failed run without dispatching Brama."}},"required":["target"]}},{"name":"probierz_analyze","description":"Parse a finished run's report (Playwright report.json or the WDIO probierz-<kind>-results.json) and inventory its media: totals, per-test status, failure reasons, and recording metadata (duration/dimensions via ffprobe, optional frame montage via ffmpeg).","inputSchema":{"type":"object","properties":{"reportPath":{"type":"string","description":"Path to the machine-readable report (from a probierz_run result)."},"artifactsDir":{"type":"string","description":"Directory to inventory for media (from a probierz_run result)."},"tool":{"type":"string","description":"playwright | wdio (inferred from the report if omitted)."},"frames":{"type":"number","description":"Extract this many frames per video (needs ffmpeg)."}},"required":["reportPath"]}},{"name":"probierz_evaluate_figure","description":"SIDE-EFFECTING: render a scientific reference/candidate pair, run deterministic geometry checks, score the declared visual rubric through the authenticated model router, and write immutable PNG evidence plus a JSON verdict.","inputSchema":{"type":"object","properties":{"referencePath":{"type":"string","description":"Reference or intermediate SVG, TeX, PDF, or raster image."},"candidatePath":{"type":"string","description":"Candidate or final SVG, TeX, PDF, or raster image."},"rubricPath":{"type":"string","description":"Optional JSON rubric; uses the scientific-figure release rubric by default."},"texPreamblePath":{"type":"string","description":"Optional LaTeX preamble lines added to the standalone wrapper used for TeX input."},"model":{"type":"string","description":"Vision-capable model ID; defaults to PROBIERZ_FIGURE_VISION_MODEL."},"agentId":{"type":"string","description":"Agent identity for subscription routes; the secret comes from PROBIERZ_MODEL_AGENT_SECRET."},"outputPath":{"type":"string","description":"Optional destination ending in .json; existing evidence is never overwritten."}},"required":["referencePath","candidatePath"]}},{"name":"probierz_evaluate_seo","description":"SIDE-EFFECTING: crawl a declared site as ordinary Chrome and Googlebot Smartphone, enforce indexability and structured-data contracts, collect mobile performance evidence, run two independent Brama content graders with conditional adjudication, ingest optional Search Console/CrUX evidence, and write an immutable signed SEO verdict.","inputSchema":{"type":"object","properties":{"appId":{"type":"string","description":"Manifest app ID; defaults to landing-page."},"baseUrl":{"type":"string","description":"Credential-free HTTPS origin or loopback HTTP URL to evaluate."},"policyPath":{"type":"string","description":"Optional SEO policy JSON; defaults to manifest seo.policy."},"briefPath":{"type":"string","description":"Optional approved landing brief JSON; defaults to manifest seo.brief."},"mode":{"type":"string","description":"pull-request, release, nightly, or production; defaults to release."},"outputPath":{"type":"string","description":"Optional immutable report destination ending in .json."},"productionEvidencePath":{"type":"string","description":"Optional Search Console and CrUX evidence JSON."},"primaryModel":{"type":"string","description":"Pinned first Brama model ID."},"secondaryModel":{"type":"string","description":"Pinned independent second Brama model ID."},"adjudicatorModel":{"type":"string","description":"Pinned Brama model used only when graders disagree."},"routerBaseUrl":{"type":"string","description":"Brama-compatible router base; defaults to STADO_MODEL_ROUTER_URL."},"agentId":{"type":"string","description":"Probierz model identity."},"privateKeyFile":{"type":"string","description":"Ed25519 PKCS#8 PEM file for release evidence signing."}},"required":["baseUrl"]}},{"name":"probierz_create_readme_gif","description":"SIDE-EFFECTING: convert one recorded journey video into a bounded, silent, looping README GIF and write a provenance sidecar with source/output SHA-256 and mandatory publication checks. Requires ffmpeg.","inputSchema":{"type":"object","properties":{"input":{"type":"string","description":"Recorded journey video path."},"output":{"type":"string","description":"Destination path ending in .gif."},"startSeconds":{"type":"number","description":"Non-negative trim offset; default 0."},"durationSeconds":{"type":"number","description":"Published clip duration; default 12, maximum 30."},"framesPerSecond":{"type":"number","description":"GIF frame rate; default 12, maximum 20."},"width":{"type":"number","description":"Output width; default 960, maximum 1200."},"force":{"type":"boolean","description":"Replace an existing GIF and sidecar."}},"required":["input","output"]}},{"name":"probierz_affected","description":"Given a change, report which run targets it could affect, so you re-run only what is relevant. Deterministic + structural (maps files to targets by package containment; agent/ or repo-root files are cross-cutting -> all targets). Provide `files` explicitly, or omit to diff the working tree against `ref` (default HEAD) via git. Read-only.","inputSchema":{"type":"object","properties":{"files":{"type":"array","items":{"type":"string"},"description":"Changed file paths (repo-relative). If given, git is not consulted."},"ref":{"type":"string","description":"git ref to diff the working tree against when `files` is omitted (default HEAD)."}},"required":[]}},{"name":"probierz_ci","description":"Change-driven pass: select affected targets, run and analyze them, then dispatch a bounded Brama repair worker for each failure unless noRepair=true. Selection and blockers stay deterministic; only the explicit repair step asks a model what to change.","inputSchema":{"type":"object","properties":{"files":{"type":"array","items":{"type":"string"},"description":"Changed file paths (repo-relative). If given, git is not consulted."},"ref":{"type":"string","description":"git ref to diff the working tree against when `files` is omitted (default HEAD)."},"appId":{"type":"string","description":"Product identifier for every selected run."},"env":{"type":"object","description":"Conditions forwarded to every selected run."},"spec":{"type":"string","description":"Optional spec filter forwarded to every selected target."},"record":{"type":"boolean","description":"Force video/trace/screenshot capture on for every run."},"force":{"type":"boolean","description":"Skip each target's preflight gate and spawn anyway."},"frames":{"type":"number","description":"Extract this many frames per recorded video (needs ffmpeg)."},"timeoutMs":{"type":"number","description":"Per-run timeout in ms."},"resourceWaitMs":{"type":"number","description":"Wait per selected run for a busy device/port lease; default 10 min, 0 fails fast."},"noRepair":{"type":"boolean","description":"Record failed runs without dispatching Brama."}},"required":[]}},{"name":"probierz_history","description":"Read deterministic E5 stability history: pass rate, infrastructure failures, duration trend, flaky tests, journeys, latest run, and last green.","inputSchema":{"type":"object","properties":{"appId":{"type":"string","description":"Product identifier (default probierz)."},"target":{"type":"string","description":"Optional target filter."},"limit":{"type":"number","description":"Maximum recent runs (default 50)."}},"required":[]}},{"name":"probierz_dashboard","description":"Project evidence for product → version → journey → surface → device → result → artifact dashboard navigation.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"limit":{"type":"number","description":"Maximum recent runs (default 500)."}},"required":["appId"]}},{"name":"probierz_matrix_plan","description":"Read the deterministic nightly or release matrix without executing it.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"profile":{"type":"string","description":"nightly or release"}},"required":["appId","profile"]}},{"name":"probierz_run_matrix","description":"HEAVY + SIDE-EFFECTING: execute every cell of a declared nightly or release matrix and return an E4 verdict.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"profile":{"type":"string","description":"nightly or release"},"release":{"type":"string","description":"Required for a release matrix."},"env":{"type":"object","description":"Secrets and exact release artifact conditions; matrix axes cannot be overridden."}},"required":["appId","profile"]}},{"name":"probierz_protect_run","description":"SIDE-EFFECTING: encrypt a complete run into an authenticated AES-256-GCM evidence bundle; optionally remove plaintext artifacts.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"runId":{"type":"string"},"kind":{"type":"string"},"keyFile":{"type":"string"},"removePlaintext":{"type":"boolean"}},"required":["appId","runId"]}},{"name":"probierz_restore_bundle","description":"SIDE-EFFECTING: authenticate and restore an encrypted evidence bundle into an empty directory.","inputSchema":{"type":"object","properties":{"file":{"type":"string"},"destination":{"type":"string"},"keyFile":{"type":"string"}},"required":["file","destination"]}},{"name":"probierz_retention","description":"Plan retention expiry; with apply=true, delete expired plaintext runs and encrypted bundles.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"at":{"type":"string","description":"Optional ISO timestamp."},"apply":{"type":"boolean"}},"required":["appId"]}},{"name":"probierz_secret_scan","description":"Scan a plaintext artifact directory for high-confidence secrets without returning secret values.","inputSchema":{"type":"object","properties":{"directory":{"type":"string"}},"required":["directory"]}},{"name":"probierz_audit","description":"Read and integrity-check access audit records, optionally filtered by app, run, or action.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"runId":{"type":"string"},"action":{"type":"string"},"limit":{"type":"number"}},"required":[]}},{"name":"probierz_source_identity","description":"Compute exact path-independent harness and app source SHA-256 identities.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"}},"required":["appId"]}},{"name":"probierz_gate_status","description":"Read pull-request and release gate activation state.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"}},"required":["appId"]}},{"name":"probierz_status","description":"Journey coverage, evidence freshness vs HEAD, untested surfaces, and pull-request merge eligibility for an app.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"baseRef":{"type":"string","description":"Default origin/main."}},"required":["appId"]}},{"name":"probierz_gate_prepush","description":"Pre-push merge gate: select affected journeys from the push diff and evaluate the newest passing runs against the exact current HEAD identity (pull-request policy).","inputSchema":{"type":"object","properties":{"repo":{"type":"string"},"appId":{"type":"string","description":"Inferred from manifest repositories when omitted."},"base":{"type":"string"},"head":{"type":"string"},"runCi":{"type":"boolean","description":"Run probierz ci <base> before evaluating."}},"required":["repo"]}},{"name":"probierz_author_spec","description":"SIDE-EFFECTING: use the authenticated Stado model router to draft one journey spec from a probe of the real app, verify it with an actual run, and keep it on green (registers the journey in the app manifest).","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"journey":{"type":"string"},"target":{"type":"string","description":"web|electron|mobile:ios|mobile:android|desktop:mac|desktop:win|tui"},"desc":{"type":"string","description":"Journey goal in one or two sentences."},"baseUrl":{"type":"string"},"appPath":{"type":"string"},"rounds":{"type":"number"}},"required":["appId","journey","target","desc"]}},{"name":"probierz_repair","description":"SIDE-EFFECTING: dispatch one bounded Brama worker at a recorded failed run. Product fixes land on a fresh published branch; spec fixes must pass the real journey before publication.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"runId":{"type":"string"},"rounds":{"type":"number","description":"One to three bounded repair rounds."},"dryRun":{"type":"boolean","description":"Write the decision brief but do not call Brama or change repositories."}},"required":["appId"]}},{"name":"probierz_author_manifest","description":"SIDE-EFFECTING: use the authenticated Stado model router to draft the whole app journey manifest from a probe and repository layout, validate it, and optionally cover every journey with author-spec.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"desc":{"type":"string","description":"What the app does, in one or two sentences."},"repositories":{"type":"array","items":{"type":"string"}},"target":{"type":"string"},"baseUrl":{"type":"string"},"appPath":{"type":"string"},"withSpecs":{"type":"boolean"}},"required":["appId","desc","repositories","target"]}},{"name":"probierz_stado_run","description":"SIDE-EFFECTING: run a target on a chosen stado host (provider/pin/spot/GPU); evidence lands back in test-results.","inputSchema":{"type":"object","properties":{"target":{"type":"string"},"appId":{"type":"string"},"spec":{"type":"string"},"host":{"type":"string","description":"stado:gcp|azure|aws|any|spot|local|t4"},"cargoRelease":{"type":"boolean","description":"Build the app binary on the worker with cargo (needs appRepo)."},"appRepo":{"type":"string"},"watch":{"type":"boolean","description":"Default true; waits for completion and fetches results."}},"required":["target","appId"]}},{"name":"probierz_stado_evaluate_seo","description":"SIDE-EFFECTING: submit the complete SEO evaluator to a Stado-selected dedicated host, materialize only the declared Brama and signing secrets, and fetch the immutable evidence bundle.","inputSchema":{"type":"object","properties":{"appId":{"type":"string","description":"Manifest app ID; defaults to landing-page."},"baseUrl":{"type":"string"},"mode":{"type":"string","description":"pull-request, release, nightly, or production."},"policyPath":{"type":"string"},"briefPath":{"type":"string"},"primaryModel":{"type":"string"},"secondaryModel":{"type":"string"},"adjudicatorModel":{"type":"string"},"agentId":{"type":"string"},"productionEvidencePath":{"type":"string"},"host":{"type":"string","description":"Dedicated Stado host; defaults to stado:mini."},"watch":{"type":"boolean","description":"Default true; waits for completion and fetches results."}},"required":["baseUrl","primaryModel","secondaryModel","adjudicatorModel"]}},{"name":"probierz_gate_evaluate","description":"Evaluate exact build, E3 evidence, coverage, matrix, encryption, secret scan, and signed receipt eligibility; appends an audit record.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"mode":{"type":"string","description":"pull-request or release"},"expectedHarnessSha":{"type":"string"},"expectedSourceSha":{"type":"string"},"runIds":{"type":"array","items":{"type":"string"}},"release":{"type":"string","description":"Required for release mode."},"receiptFile":{"type":"string","description":"Required signed evidence receipt for release mode."},"trustedPublicKeyFile":{"type":"string"},"expectedFingerprint":{"type":"string"}},"required":["appId","mode","expectedHarnessSha","expectedSourceSha","runIds"]}},{"name":"probierz_gate_enforce","description":"Enforce an activated gate against current evidence; pending-green gates fail closed.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"mode":{"type":"string","description":"pull-request or release"},"expectedHarnessSha":{"type":"string"},"expectedSourceSha":{"type":"string"},"runIds":{"type":"array","items":{"type":"string"}},"release":{"type":"string","description":"Required for release mode."},"receiptFile":{"type":"string","description":"Required signed evidence receipt for release mode."},"trustedPublicKeyFile":{"type":"string"},"expectedFingerprint":{"type":"string"}},"required":["appId","mode","expectedHarnessSha","expectedSourceSha","runIds"]}},{"name":"probierz_gate_activate","description":"SIDE-EFFECTING: atomically activate a gate only after all green evidence requirements pass.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"mode":{"type":"string","description":"pull-request or release"},"expectedHarnessSha":{"type":"string"},"expectedSourceSha":{"type":"string"},"runIds":{"type":"array","items":{"type":"string"}},"release":{"type":"string","description":"Required for release mode."},"receiptFile":{"type":"string","description":"Required signed evidence receipt for release mode."},"trustedPublicKeyFile":{"type":"string"},"expectedFingerprint":{"type":"string"}},"required":["appId","mode","expectedHarnessSha","expectedSourceSha","runIds"]}},{"name":"probierz_compare_runs","description":"Deterministically compare status, duration, tests, evidence, build identity, and artifact hashes between two run IDs.","inputSchema":{"type":"object","properties":{"appId":{"type":"string","description":"Product identifier (default probierz)."},"leftRunId":{"type":"string"},"rightRunId":{"type":"string"}},"required":["leftRunId","rightRunId"]}},{"name":"probierz_last_green","description":"Return the newest passing run for a product, optional target, and optional journey.","inputSchema":{"type":"object","properties":{"appId":{"type":"string","description":"Product identifier (default probierz)."},"target":{"type":"string"},"journey":{"type":"string"}},"required":[]}},{"name":"probierz_create_receipt","description":"SIDE-EFFECTING: secret-scan evidence, verify exact source/build/artifact provenance, and sign a release receipt with immutable journey identities and report-typed publication media.","inputSchema":{"type":"object","properties":{"appId":{"type":"string"},"release":{"type":"string"},"expectedHarnessSha":{"type":"string"},"expectedSourceSha":{"type":"string"},"runIds":{"type":"array","items":{"type":"string"}},"requiredJourneys":{"type":"array","items":{"type":"string"}},"minimumEvidence":{"type":"string","description":"Default E3."},"privateKeyFile":{"type":"string","description":"Defaults to PROBIERZ_RECEIPT_PRIVATE_KEY_FILE."}},"required":["appId","release","expectedHarnessSha","expectedSourceSha","runIds"]}},{"name":"probierz_verify_receipt","description":"Verify receipt payload hash and Ed25519 signature against an explicit trusted public key or fingerprint.","inputSchema":{"type":"object","properties":{"file":{"type":"string"},"trustedPublicKeyFile":{"type":"string"},"expectedFingerprint":{"type":"string"}},"required":["file"]}},{"name":"probierz_create_publication_manifest","description":"SIDE-EFFECTING: verify a signed receipt, current source, secret scan, evidence hashes, driver capability, redaction review, and immutable storage registrations before emitting a deterministic first-use publication manifest.","inputSchema":{"type":"object","properties":{"receiptFile":{"type":"string"},"attemptId":{"type":"string"},"journeyId":{"type":"string"},"assets":{"type":"array","items":{"type":"object","properties":{"file":{"type":"string"},"kind":{"type":"string","enum":["screenshot","recording","trace"]},"storageUrl":{"type":"string"},"contentSha256":{"type":"string"},"redactionStatus":{"type":"string","enum":["verified_redacted","not_applicable"]},"verifiedAt":{"type":"string"}},"required":["file","storageUrl","contentSha256","redactionStatus","verifiedAt"],"additionalProperties":false}},"trustedPublicKeyFile":{"type":"string"},"expectedFingerprint":{"type":"string"}},"required":["receiptFile","attemptId","journeyId","assets"]}},{"name":"probierz_start_run","description":"HEAVY + SIDE-EFFECTING: start a real run asynchronously and return its runId immediately. Poll with probierz_run_status; cancel with probierz_cancel_run.","inputSchema":{"type":"object","properties":{"target":{"type":"string"},"record":{"type":"boolean"},"appId":{"type":"string"},"env":{"type":"object"},"timeoutMs":{"type":"number"},"resourceWaitMs":{"type":"number","description":"Wait this long for a busy device/port lease; 0 fails fast."},"frames":{"type":"number"},"analyze":{"type":"boolean"},"force":{"type":"boolean"},"spec":{"type":"string"},"noRepair":{"type":"boolean","description":"Record a failed run without dispatching Brama."}},"required":["target"]}},{"name":"probierz_run_status","description":"Return queued/running/blocked/passed/failed/canceled state for an asynchronous run.","inputSchema":{"type":"object","properties":{"runId":{"type":"string"}},"required":["runId"]}},{"name":"probierz_cancel_run","description":"Cancel an asynchronous run and terminate its complete spawned process tree.","inputSchema":{"type":"object","properties":{"runId":{"type":"string"}},"required":["runId"]}},{"name":"probierz_get_result","description":"Return the completed normalized result and evidence for an asynchronous run.","inputSchema":{"type":"object","properties":{"runId":{"type":"string"}},"required":["runId"]}},{"name":"probierz_list_artifacts","description":"List run-scoped evidence artifacts for a completed asynchronous run.","inputSchema":{"type":"object","properties":{"runId":{"type":"string"}},"required":["runId"]}},{"name":"probierz_get_artifact","description":"Read one run-scoped artifact up to 5 MiB as base64; path traversal is rejected.","inputSchema":{"type":"object","properties":{"runId":{"type":"string"},"file":{"type":"string","description":"Run-relative artifact path."}},"required":["runId","file"]}}]"###;

const MAX_ARTIFACT_BYTES: u64 = 5 * 1024 * 1024;

struct Job {
    run_id: String,
    status: &'static str,
    target: String,
    app_id: String,
    spec: Option<String>,
    record: bool,
    created_at: String,
    started_at: Option<String>,
    completed_at: Option<String>,
    error: Option<String>,
    result: Option<Value>,
    cancel_requested: bool,
    child: Option<Arc<Mutex<Child>>>,
}

#[derive(Default)]
struct Control {
    jobs: Mutex<HashMap<String, Arc<Mutex<Job>>>>,
}

impl Control {
    fn start(self: &Arc<Self>, args: &Map<String, Value>) -> Result<Value, String> {
        let target = non_empty(args.get("target"), "target")?.to_string();
        let run_id = new_run_id();
        let job = Arc::new(Mutex::new(Job {
            run_id: run_id.clone(),
            status: "queued",
            target,
            app_id: args
                .get("appId")
                .and_then(Value::as_str)
                .unwrap_or("probierz")
                .to_string(),
            spec: args.get("spec").and_then(Value::as_str).map(str::to_string),
            record: args.get("record").and_then(Value::as_bool).unwrap_or(false),
            created_at: now_iso(),
            started_at: None,
            completed_at: None,
            error: None,
            result: None,
            cancel_requested: false,
            child: None,
        }));
        let answer = {
            let job = job
                .lock()
                .map_err(|_| "control state unavailable".to_string())?;
            public_job(&job)
        };
        self.jobs
            .lock()
            .map_err(|_| "control state unavailable".to_string())?
            .insert(run_id, Arc::clone(&job));

        let control_args = args.clone();
        thread::spawn(move || execute_job(job, control_args));
        Ok(answer)
    }

    fn job(&self, run_id: &str) -> Result<Arc<Mutex<Job>>, String> {
        self.jobs
            .lock()
            .map_err(|_| "control state unavailable".to_string())?
            .get(run_id)
            .cloned()
            .ok_or_else(|| format!("unknown runId: {run_id}"))
    }

    fn status(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let job = self.job(run_id)?;
        let answer = {
            let job = job
                .lock()
                .map_err(|_| "control state unavailable".to_string())?;
            public_job(&job)
        };
        Ok(answer)
    }

    fn cancel(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let job = self.job(run_id)?;
        let (answer, child) = {
            let mut job = job
                .lock()
                .map_err(|_| "control state unavailable".to_string())?;
            if matches!(job.status, "passed" | "failed" | "blocked" | "canceled") {
                let mut answer = public_job(&job);
                answer
                    .as_object_mut()
                    .expect("job answer")
                    .insert("cancelRequested".into(), Value::Bool(false));
                return Ok(answer);
            }
            job.cancel_requested = true;
            let mut answer = public_job(&job);
            answer
                .as_object_mut()
                .expect("job answer")
                .insert("cancelRequested".into(), Value::Bool(true));
            (answer, job.child.clone())
        };
        if let Some(child) = child {
            terminate_tree(&child);
        }
        Ok(answer)
    }

    fn result(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let job = self.job(run_id)?;
        let job = job
            .lock()
            .map_err(|_| "control state unavailable".to_string())?;
        let mut answer = public_job(&job);
        answer
            .as_object_mut()
            .expect("job answer")
            .insert("result".into(), job.result.clone().unwrap_or(Value::Null));
        Ok(answer)
    }

    fn list_artifacts(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let job = self.job(run_id)?;
        let root = artifact_root(&job)?;
        let mut pending = vec![root.clone()];
        let mut artifacts = Vec::new();
        while let Some(directory) = pending.pop() {
            let entries = fs::read_dir(&directory).map_err(|error| error.to_string())?;
            for entry in entries {
                let entry = entry.map_err(|error| error.to_string())?;
                let metadata = entry.file_type().map_err(|error| error.to_string())?;
                if metadata.is_dir() {
                    pending.push(entry.path());
                } else if metadata.is_file() {
                    let bytes = entry.metadata().map_err(|error| error.to_string())?.len();
                    let file = entry
                        .path()
                        .strip_prefix(&root)
                        .map_err(|error| error.to_string())?
                        .to_string_lossy()
                        .into_owned();
                    artifacts.push(json!({ "file": file, "bytes": bytes }));
                }
            }
        }
        artifacts.sort_by(|left, right| {
            left.get("file")
                .and_then(Value::as_str)
                .cmp(&right.get("file").and_then(Value::as_str))
        });
        Ok(json!({ "runId": run_id, "artifacts": artifacts }))
    }

    fn get_artifact(&self, args: &Map<String, Value>) -> Result<Value, String> {
        let run_id = non_empty(args.get("runId"), "runId")?;
        let relative = non_empty(args.get("file"), "file")
            .map_err(|_| "file must be a non-empty relative path".to_string())?;
        let job = self.job(run_id)?;
        let root = artifact_root(&job)?;
        let relative_path = Path::new(relative);
        if relative_path.is_absolute()
            || relative_path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err("artifact path escapes the run directory".to_string());
        }
        let lexical = root.join(relative_path);
        let metadata = fs::symlink_metadata(&lexical).map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!("artifact is not a file: {relative}"));
        }
        if metadata.len() > MAX_ARTIFACT_BYTES {
            return Err(format!(
                "artifact exceeds {MAX_ARTIFACT_BYTES} byte inline limit"
            ));
        }
        let file = fs::canonicalize(&lexical).map_err(|error| error.to_string())?;
        if file != root && !file.starts_with(&root) {
            return Err("artifact path escapes the run directory".to_string());
        }
        let content = fs::read(&file).map_err(|error| error.to_string())?;
        let file = lexical
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .into_owned();
        Ok(json!({
            "runId": run_id,
            "file": file,
            "bytes": metadata.len(),
            "encoding": "base64",
            "content": BASE64.encode(content),
        }))
    }

    fn shutdown(&self) {
        let jobs = self
            .jobs
            .lock()
            .map(|jobs| jobs.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for job in jobs {
            let child = job.lock().ok().and_then(|mut job| {
                if matches!(job.status, "queued" | "running") {
                    job.cancel_requested = true;
                    job.child.clone()
                } else {
                    None
                }
            });
            if let Some(child) = child {
                terminate_tree(&child);
            }
        }
    }
}

fn new_run_id() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let uuid = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}-{}",
        now_iso().replace(':', "-").replace('.', "-"),
        &uuid[0..8],
        &uuid[8..12],
        &uuid[12..16],
        &uuid[16..20],
        &uuid[20..32]
    )
}

fn public_job(job: &Job) -> Value {
    let artifacts_dir = job
        .result
        .as_ref()
        .and_then(|result| result.get("artifactsDir"))
        .cloned()
        .unwrap_or(Value::Null);
    json!({
        "runId": job.run_id,
        "status": job.status,
        "target": job.target,
        "appId": job.app_id,
        "spec": job.spec,
        "record": job.record,
        "createdAt": job.created_at,
        "startedAt": job.started_at,
        "completedAt": job.completed_at,
        "error": job.error,
        "artifactsDir": artifacts_dir,
    })
}

fn execute_job(job: Arc<Mutex<Job>>, mut args: Map<String, Value>) {
    {
        let Ok(mut job) = job.lock() else {
            return;
        };
        job.status = "running";
        job.started_at = Some(now_iso());
        if job.cancel_requested {
            job.status = "canceled";
            job.completed_at = Some(now_iso());
            return;
        }
    }

    let analyze = args
        .remove("analyze")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let arguments = match route("probierz_run", &args) {
        Ok(mut arguments) => {
            if !analyze {
                arguments.push("--no-analyze".to_string());
            }
            arguments
        }
        Err(error) => {
            finish_job_error(&job, error);
            return;
        }
    };
    let mut command = Command::new(probierz_binary());
    command
        .arg("--harness")
        .arg(harness_root())
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            finish_job_error(&job, format!("cannot run probierz: {error}"));
            return;
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(child));
    {
        let Ok(mut job) = job.lock() else {
            terminate_tree(&child);
            return;
        };
        job.child = Some(Arc::clone(&child));
        if job.cancel_requested {
            terminate_tree(&child);
        }
    }
    let stdout_reader = thread::spawn(move || read_pipe(stdout));
    let stderr_reader = thread::spawn(move || read_pipe(stderr));
    let status = loop {
        let waited = child
            .lock()
            .map_err(|_| "run process state unavailable".to_string())
            .and_then(|mut child| child.try_wait().map_err(|error| error.to_string()));
        match waited {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => break Err(error),
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    finish_job(&job, status, stdout, stderr);
}

fn read_pipe(pipe: Option<impl Read>) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut bytes);
    }
    bytes
}

fn finish_job(
    job: &Arc<Mutex<Job>>,
    status: Result<ExitStatus, String>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
) {
    let Ok(mut job) = job.lock() else {
        return;
    };
    job.child = None;
    job.completed_at = Some(now_iso());
    if job.cancel_requested {
        job.status = "canceled";
        return;
    }
    let process_status = match status {
        Ok(status) => status,
        Err(error) => {
            job.status = "failed";
            job.error = Some(error);
            return;
        }
    };
    if !stdout.is_empty() {
        match serde_json::from_slice::<Value>(&stdout) {
            Ok(result) => {
                job.status = if result.get("skipped").and_then(Value::as_bool) == Some(true) {
                    "blocked"
                } else if result.get("canceled").and_then(Value::as_bool) == Some(true) {
                    "canceled"
                } else if result.get("passed").and_then(Value::as_bool) == Some(true) {
                    "passed"
                } else {
                    "failed"
                };
                job.result = Some(result);
                return;
            }
            Err(error) => {
                job.error = Some(format!("probierz returned invalid JSON: {error}"));
                job.status = "failed";
                return;
            }
        }
    }
    let stderr = String::from_utf8_lossy(&stderr);
    job.error = Some(if stderr.trim().is_empty() {
        format!(
            "exit {}",
            process_status
                .code()
                .map_or_else(|| "null".to_string(), |code| code.to_string())
        )
    } else {
        stderr.trim().to_string()
    });
    job.status = "failed";
}

fn finish_job_error(job: &Arc<Mutex<Job>>, error: String) {
    if let Ok(mut job) = job.lock() {
        job.status = if job.cancel_requested {
            "canceled"
        } else {
            "failed"
        };
        job.error = Some(error);
        job.completed_at = Some(now_iso());
    }
}

fn artifact_root(job: &Arc<Mutex<Job>>) -> Result<PathBuf, String> {
    let job = job
        .lock()
        .map_err(|_| "control state unavailable".to_string())?;
    let root = job
        .result
        .as_ref()
        .and_then(|result| result.get("artifactsDir"))
        .and_then(Value::as_str)
        .filter(|root| !root.is_empty())
        .ok_or_else(|| format!("artifacts unavailable for runId: {}", job.run_id))?;
    let root = fs::canonicalize(root)
        .map_err(|_| format!("artifacts unavailable for runId: {}", job.run_id))?;
    if !root.is_dir() {
        return Err(format!("artifacts unavailable for runId: {}", job.run_id));
    }
    Ok(root)
}

fn terminate_tree(child: &Arc<Mutex<Child>>) {
    let Ok(mut child) = child.lock() else {
        return;
    };
    let pid = child.id();
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        let mut processes: HashMap<u32, Vec<u32>> = HashMap::new();
        if let Ok(output) = Command::new("/bin/ps")
            .args(["-axo", "pid=,ppid="])
            .output()
        {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let mut columns = line.split_whitespace();
                if let (Some(process), Some(parent)) = (columns.next(), columns.next()) {
                    if let (Ok(process), Ok(parent)) =
                        (process.parse::<u32>(), parent.parse::<u32>())
                    {
                        processes.entry(parent).or_default().push(process);
                    }
                }
            }
        }
        fn descendants(pid: u32, processes: &HashMap<u32, Vec<u32>>, output: &mut Vec<u32>) {
            for child in processes.get(&pid).into_iter().flatten() {
                descendants(*child, processes, output);
            }
            output.push(pid);
        }
        let mut tree = Vec::new();
        descendants(pid, &processes, &mut tree);
        let ids = tree.iter().map(u32::to_string).collect::<Vec<_>>();
        let _ = Command::new("/bin/kill")
            .arg("-TERM")
            .args(&ids)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        thread::sleep(Duration::from_millis(100));
        let _ = Command::new("/bin/kill")
            .arg("-KILL")
            .args(&ids)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

fn send(value: &Value) {
    let mut stdout = io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, value);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

fn harness_root() -> PathBuf {
    if let Some(root) = std::env::var_os("PROBIERZ_HARNESS") {
        return PathBuf::from(root);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")))
        .to_path_buf()
}

fn probierz_binary() -> PathBuf {
    if let Some(binary) = std::env::var_os("PROBIERZ_BIN") {
        return PathBuf::from(binary);
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("probierz")))
        .unwrap_or_else(|| PathBuf::from("probierz"))
}

fn non_empty<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a str, String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{name} must be a non-empty string"))
}

fn kebab(name: &str) -> String {
    let mut result = String::new();
    for character in name.chars() {
        if character.is_ascii_uppercase() {
            result.push('-');
            result.push(character.to_ascii_lowercase());
        } else {
            result.push(character);
        }
    }
    result
}

fn append_flag(arguments: &mut Vec<String>, name: &str, value: &Value) {
    let flag = format!("--{}", kebab(name));
    match value {
        Value::Bool(true) => arguments.push(flag),
        Value::Bool(false) | Value::Null => {}
        Value::String(text) => {
            arguments.push(flag);
            arguments.push(text.clone());
        }
        Value::Number(number) => {
            arguments.push(flag);
            arguments.push(number.to_string());
        }
        Value::Array(items) => {
            for item in items {
                append_flag(arguments, name, item);
            }
        }
        Value::Object(_) => {}
    }
}

fn route(name: &str, args: &Map<String, Value>) -> Result<Vec<String>, String> {
    let mut output = Vec::new();
    let mut positional: Vec<&str> = Vec::new();
    let command = match name {
        "probierz_list_surfaces" => "list",
        "probierz_list_specs" => {
            if args.contains_key("surface") {
                positional.push("surface");
            }
            "specs"
        }
        "probierz_describe_spec" => {
            positional.push("spec");
            "describe"
        }
        "probierz_run_command" => {
            positional.push("target");
            "cmd"
        }
        "probierz_source_identity" => {
            positional.push("appId");
            "source-identity"
        }
        "probierz_check" => {
            positional.push("target");
            "check"
        }
        "probierz_setup" => {
            positional.push("target");
            "setup"
        }
        "probierz_run" => {
            positional.push("target");
            "run"
        }
        "probierz_analyze" => {
            positional.push("reportPath");
            "analyze"
        }
        "probierz_evaluate_figure" => "figure-evaluate",
        "probierz_evaluate_seo" => "seo-evaluate",
        "probierz_create_readme_gif" => {
            positional.push("input");
            "readme-gif"
        }
        "probierz_affected" => "affected",
        "probierz_ci" => "ci",
        "probierz_history" => "history",
        "probierz_dashboard" => {
            positional.push("appId");
            "dashboard"
        }
        "probierz_matrix_plan" => {
            positional.extend(["appId", "profile"]);
            "matrix-plan"
        }
        "probierz_run_matrix" => {
            positional.extend(["appId", "profile"]);
            "matrix"
        }
        "probierz_protect_run" => {
            positional.extend(["appId", "runId"]);
            "protect"
        }
        "probierz_restore_bundle" => {
            positional.extend(["file", "destination"]);
            "restore"
        }
        "probierz_retention" => {
            positional.push("appId");
            "retention"
        }
        "probierz_secret_scan" => {
            positional.push("directory");
            "secret-scan"
        }
        "probierz_audit" => "audit",
        "probierz_gate_status" => {
            positional.push("appId");
            "gate-status"
        }
        "probierz_status" => {
            positional.push("appId");
            "status"
        }
        "probierz_gate_prepush" => {
            positional.push("repo");
            "gate-prepush"
        }
        "probierz_author_spec" => {
            positional.extend(["appId", "journey"]);
            "author-spec"
        }
        "probierz_repair" => {
            positional.push("appId");
            "repair"
        }
        "probierz_author_manifest" => {
            positional.push("appId");
            "author-manifest"
        }
        "probierz_stado_run" => {
            positional.push("target");
            "stado"
        }
        "probierz_stado_evaluate_seo" => "stado-seo",
        "probierz_gate_evaluate" => {
            positional.push("appId");
            "gate-evaluate"
        }
        "probierz_gate_enforce" => {
            positional.push("appId");
            "gate-enforce"
        }
        "probierz_gate_activate" => {
            positional.push("appId");
            "gate-activate"
        }
        "probierz_compare_runs" => {
            positional.extend(["leftRunId", "rightRunId"]);
            "compare"
        }
        "probierz_last_green" => "last-green",
        "probierz_create_receipt" => {
            positional.extend(["appId", "release"]);
            "receipt-create"
        }
        "probierz_verify_receipt" => {
            positional.push("file");
            "receipt-verify"
        }
        "probierz_create_publication_manifest" => "publication-create",
        _ => return Err(format!("unknown tool: {name}")),
    };
    output.push(command.to_string());
    for key in &positional {
        if let Some(value) = args.get(*key) {
            output.push(non_empty(Some(value), key)?.to_string());
        }
    }
    for (key, value) in args {
        if positional.contains(&key.as_str()) {
            continue;
        }
        if key == "env" {
            if let Some(environment) = value.as_object() {
                for (name, value) in environment {
                    let text = value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string());
                    output.push(format!("{name}={text}"));
                }
            }
            continue;
        }
        let cli_key = match key.as_str() {
            "referencePath" => "reference",
            "candidatePath" => "candidate",
            "rubricPath" => "rubric",
            "outputPath" | "output" => "out",
            "texPreamblePath" => "tex-preamble",
            "routerBaseUrl" => "router-url",
            "policyPath" => "policy",
            "briefPath" => "brief",
            "productionEvidencePath" => "production-evidence",
            "privateKeyFile" => "private-key-file",
            "repositories" => "repo",
            "withSpecs" => "specs",
            "appId" => "app",
            "baseRef" => "base",
            "leftRunId" => "left",
            "rightRunId" => "right",
            "runIds" => "run",
            "cargoRelease" => "cargo-release",
            "appRepo" => "app-repo",
            "noRepair" => "no-repair",
            "timeoutMs" => "timeout",
            "resourceWaitMs" => "resource-wait",
            "startSeconds" => "start",
            "durationSeconds" => "duration",
            "framesPerSecond" => "fps",
            other => other,
        };
        append_flag(&mut output, cli_key, value);
    }
    Ok(output)
}

fn tool_answer(value: Value) -> Result<Value, String> {
    let pretty = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    Ok(json!({ "content": [{ "type": "text", "text": pretty }] }))
}

fn call_tool(
    control: &Arc<Control>,
    name: &str,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    // The control tools are stateful for the lifetime of this MCP server.
    // Their worker still enters through `probierz run`, the product's one
    // execution path; only supervision and artifact reads live here.
    let controlled = match name {
        "probierz_start_run" => Some(control.start(args)),
        "probierz_run_status" => Some(control.status(args)),
        "probierz_cancel_run" => Some(control.cancel(args)),
        "probierz_get_result" => Some(control.result(args)),
        "probierz_list_artifacts" => Some(control.list_artifacts(args)),
        "probierz_get_artifact" => Some(control.get_artifact(args)),
        _ => None,
    };
    if let Some(result) = controlled {
        return tool_answer(result?);
    }

    // Read-only and side-effecting operations share transport, not authority:
    // no operation runs until this explicit call is routed. Discovery commands
    // remain the CLI's static, non-executing surfaces.
    let arguments = route(name, args)?;
    let output = Command::new(probierz_binary())
        .arg("--harness")
        .arg(harness_root())
        .args(&arguments)
        .output()
        .map_err(|error| format!("cannot run probierz: {error}"))?;
    if !output.stdout.is_empty() {
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("probierz returned invalid JSON: {error}"))?;
        return tool_answer(value);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr
        .lines()
        .last()
        .filter(|line| !line.is_empty())
        .unwrap_or("probierz command failed");
    Err(message.to_string())
}

fn handle(request: Value, tools: &Value, control: &Arc<Control>) {
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return;
    };
    if request.get("id").is_none() {
        return;
    }
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "probierz", "version": env!("CARGO_PKG_VERSION") },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools })),
        "tools/call" => {
            let params = request.get("params").and_then(Value::as_object);
            let name = params.and_then(|value| non_empty(value.get("name"), "name").ok());
            match name {
                Some(name) => {
                    let empty = Map::new();
                    let args = params
                        .and_then(|value| value.get("arguments"))
                        .and_then(Value::as_object)
                        .unwrap_or(&empty);
                    call_tool(control, name, args)
                }
                None => Err("name must be a non-empty string".to_string()),
            }
        }
        _ => {
            send(
                &json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("method not found: {method}") } }),
            );
            return;
        }
    };
    match result {
        Ok(result) => send(&json!({ "jsonrpc": "2.0", "id": id, "result": result })),
        Err(message) => {
            let code = if message.starts_with("unknown tool:") {
                -32601
            } else {
                -32000
            };
            send(
                &json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
            )
        }
    }
}

fn main() {
    let tools: Value = match serde_json::from_str(TOOLS_JSON) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("probierz-mcp tool contract is invalid: {error}");
            std::process::exit(1);
        }
    };
    let control = Arc::new(Control::default());
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else {
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(request) => handle(request, &tools, &control),
            Err(_) => send(
                &json!({ "jsonrpc": "2.0", "id": Value::Null, "error": { "code": -32700, "message": "parse error" } }),
            ),
        }
    }
    control.shutdown();
}
