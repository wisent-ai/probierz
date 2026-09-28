//! The jeden CLI against a reachable Brama control plane (BRAMA_URL, or the
//! one in ~/.jeden/.env). Each journey first proves Brama answers /health and
//! names the unreachable address otherwise.

use std::fs;
use std::path::PathBuf;

use regex::Regex;

use super::constants::RUN_MODEL;
use super::succeeded;
use crate::specs::{self, tui::common};

fn brama(context: &specs::Context) -> Result<(), String> {
    let from_file = || {
        let env = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".jeden/.env");
        fs::read_to_string(env).ok().and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("BRAMA_URL="))
                .map(|value| {
                    value
                        .trim()
                        .trim_matches(|c| c == '"' || c == '\'')
                        .to_string()
                })
        })
    };
    let url = context
        .optional("BRAMA_URL")
        .or_else(from_file)
        .ok_or("BRAMA_URL is required (set it, or in ~/.jeden/.env): these journeys need the Brama control plane")?;
    let health = format!("{}/health", url.trim_end_matches('/'));
    ureq::get(&health)
        .call()
        .map(drop)
        .map_err(|error| format!("Brama is unreachable at {health}: {error}"))
}

/// jeden-cli-run: `jeden run` prints the model's answer.
pub fn run(context: &specs::Context) -> Result<(), String> {
    brama(context)?;
    let out = succeeded(
        context,
        &[
            "run",
            "Respond exactly: OK",
            "--model",
            RUN_MODEL,
            "--max-steps",
            "1",
        ],
        None,
    )?;
    common::contains(
        &out,
        "OK",
        format!("jeden run did not print the model's answer:\n{out}"),
    )
}

/// jeden-cli-model-catalog: the piped /model lists routes with availability and groups models by provider.
pub fn model_catalog(context: &specs::Context) -> Result<(), String> {
    brama(context)?;
    let out = succeeded(context, &[], Some("/model\n"))?;
    common::contains(
        &out,
        "- any [AUTO]",
        format!("/model lists no automatic route:\n{out}"),
    )?;
    common::contains(
        &out,
        "Show all",
        format!("/model offers no Show all row:\n{out}"),
    )?;
    if !(out.contains("[AVAILABLE]") || out.contains("[ACTIVE]")) {
        return Err(format!("/model shows no availability badge:\n{out}"));
    }
    // Piped text has no panes, so the provider grouping arrives as section
    // headers with counts.
    let providers = Regex::new(r"── \S+ \(\d+\) ──").map_err(|error| error.to_string())?;
    let catalog = Regex::new(r"○ catalog — \d+ models? · no credentials")
        .map_err(|error| error.to_string())?;
    if !providers.is_match(&out) || !catalog.is_match(&out) {
        return Err(format!(
            "/model does not group models by provider with counts:\n{out}"
        ));
    }
    Ok(())
}

/// jeden-cli-token: `jeden token` redacts by default, reveals on demand, and /token never reveals.
pub fn token(context: &specs::Context) -> Result<(), String> {
    brama(context)?;
    let redacted = succeeded(context, &["token"], None)?;
    common::contains(
        &redacted,
        "…",
        format!("jeden token did not redact:\n{redacted}"),
    )?;
    let revealed = succeeded(context, &["token", "--reveal"], None)?;
    let revealed = revealed.trim();
    if revealed.len() <= 16 || revealed.contains('\n') {
        return Err("jeden token --reveal did not print one full credential".into());
    }
    common::excludes(
        &redacted,
        revealed,
        "jeden token printed the credential it should redact",
    )?;
    let slash = succeeded(context, &[], Some("/token\n"))?;
    common::contains(
        &slash,
        "redacted",
        "/token does not say it redacted the credential",
    )?;
    common::excludes(&slash, revealed, "/token revealed the credential")
}

/// jeden-cli-usage: /usage shows provider usage or says the quota is unavailable.
pub fn usage(context: &specs::Context) -> Result<(), String> {
    brama(context)?;
    let out = succeeded(context, &[], Some("/usage\n"))?;
    common::contains(
        &out,
        "Provider usage",
        format!("/usage shows no provider usage:\n{out}"),
    )?;
    if !(out.contains("[✔]") || out.contains("quota unavailable")) {
        return Err(format!(
            "/usage reports neither a live provider nor an unavailable quota:\n{out}"
        ));
    }
    Ok(())
}

/// jeden-cli-doctor: `jeden doctor` reports healthy.
pub fn doctor(context: &specs::Context) -> Result<(), String> {
    brama(context)?;
    let out = succeeded(context, &["doctor"], None)?;
    common::contains(
        &out,
        "\"healthy\":true",
        format!("jeden doctor does not report healthy:\n{out}"),
    )
}
