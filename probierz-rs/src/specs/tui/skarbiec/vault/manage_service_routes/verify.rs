//! Reading the routes back: the sound list, the narrowed list and a clean
//! verify; then two routes the vault cannot deliver, the rows that report it,
//! and the verify that exits non-zero naming each broken resource.

use std::time::Duration;

use serde_json::{json, Value};

use crate::specs::tui::skarbiec::fixture;

use super::journey::{
    ok_json, route_row, run_command, tail, Journey, EMAIL_RESOURCE, LOGIN_ITEM,
    MISSING_FIELD_RESOURCE, MISSING_ITEM_RESOURCE, PASSWORD_RESOURCE,
};

/// Lists and verifies the two sound routes; returns the verify answer.
pub(super) fn verify_sound(journey: &mut Journey<'_>) -> Result<Value, String> {
    let sound_list = ok_json(
        &mut journey.shell,
        journey.binary,
        &["routes", "list"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes list",
        &mut journey.executed,
    )?;
    fixture::ensure(
        sound_list["consumer"].is_null(),
        format!("unnarrowed routes list has a consumer: {sound_list}"),
    )?;
    fixture::ensure(
        sound_list["routes"]
            == json!([
                {"resource":EMAIL_RESOURCE,"item":LOGIN_ITEM,"field":"username","item_present":true,"field_present":true},
                {"resource":PASSWORD_RESOURCE,"item":LOGIN_ITEM,"field":"password","item_present":true,"field_present":true}
            ]),
        format!("sound routes list is wrong: {sound_list}"),
    )?;
    let narrowed = ok_json(
        &mut journey.shell,
        journey.binary,
        &["routes", "list", "dash.cloudflare.com/password"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes list dash.cloudflare.com/password",
        &mut journey.executed,
    )?;
    fixture::ensure(
        narrowed["consumer"] == "dash.cloudflare.com/password",
        format!("narrowed consumer is wrong: {narrowed}"),
    )?;
    fixture::ensure(
        narrowed["routes"].as_array().map(Vec::len) == Some(1)
            && narrowed["routes"][0]["resource"] == PASSWORD_RESOURCE,
        format!("narrowed routes are wrong: {narrowed}"),
    )?;
    let sound_verify = ok_json(
        &mut journey.shell,
        journey.binary,
        &["routes", "verify"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes verify",
        &mut journey.executed,
    )?;
    fixture::ensure(
        sound_verify == json!({"checked":2,"broken":[]}),
        format!("sound verify answer is wrong: {sound_verify}"),
    )?;
    Ok(sound_verify)
}

/// Adds two routes the vault cannot deliver and proves list and verify say so;
/// returns the broken verify report.
pub(super) fn verify_broken(journey: &mut Journey<'_>) -> Result<Value, String> {
    let broken_item = ok_json(
        &mut journey.shell,
        journey.binary,
        &[
            "routes",
            "add",
            "--resource",
            MISSING_ITEM_RESOURCE,
            "--item",
            "absent-login-item",
            "--field",
            "username",
            "--reason",
            "journey: route naming an item this vault does not hold",
        ],
        &[],
        Duration::from_secs(60),
        "skarbiec routes add missing item",
        &mut journey.executed,
    )?;
    fixture::ensure(
        broken_item["added"] == true,
        format!("broken item route was not added: {broken_item}"),
    )?;
    let broken_field = ok_json(
        &mut journey.shell,
        journey.binary,
        &[
            "routes",
            "add",
            "--resource",
            MISSING_FIELD_RESOURCE,
            "--item",
            LOGIN_ITEM,
            "--field",
            "totp_secret",
            "--reason",
            "journey: route naming a field this item does not carry",
        ],
        &[],
        Duration::from_secs(60),
        "skarbiec routes add missing field",
        &mut journey.executed,
    )?;
    fixture::ensure(
        broken_field["added"] == true,
        format!("broken field route was not added: {broken_field}"),
    )?;
    let broken_list = ok_json(
        &mut journey.shell,
        journey.binary,
        &["routes", "list"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes list",
        &mut journey.executed,
    )?;
    let broken_rows = broken_list["routes"]
        .as_array()
        .ok_or_else(|| "routes list returned no routes array".to_string())?;
    fixture::ensure(
        broken_rows.len() == 4,
        format!("broken routes list has {} rows", broken_rows.len()),
    )?;
    let item_row = route_row(broken_rows, MISSING_ITEM_RESOURCE)?;
    fixture::ensure(
        item_row
            == &json!({"resource":MISSING_ITEM_RESOURCE,"item":"absent-login-item","field":"username","item_present":false,"field_present":false}),
        format!("missing-item row is wrong: {item_row}"),
    )?;
    let field_row = route_row(broken_rows, MISSING_FIELD_RESOURCE)?;
    fixture::ensure(
        field_row
            == &json!({"resource":MISSING_FIELD_RESOURCE,"item":LOGIN_ITEM,"field":"totp_secret","item_present":true,"field_present":false}),
        format!("missing-field row is wrong: {field_row}"),
    )?;
    fixture::ensure(
        route_row(broken_rows, EMAIL_RESOURCE)?["field_present"] == true,
        "email route no longer resolves",
    )?;
    fixture::ensure(
        route_row(broken_rows, PASSWORD_RESOURCE)?["field_present"] == true,
        "password route no longer resolves",
    )?;

    let broken_verify = run_command(
        &mut journey.shell,
        journey.binary,
        &["routes", "verify"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes verify",
        &mut journey.executed,
    )?;
    fixture::ensure(
        broken_verify.status != 0,
        "routes verify passed a table that cannot deliver",
    )?;
    let broken_report = fixture::parse_json(&broken_verify.output, || {
        "skarbiec routes verify emitted no JSON".to_string()
    })?;
    fixture::ensure(
        broken_report["checked"] == 4,
        format!("broken verify checked the wrong count: {broken_report}"),
    )?;
    let broken = broken_report["broken"]
        .as_array()
        .ok_or_else(|| "broken verify returned no broken array".to_string())?;
    fixture::ensure(broken.iter().any(|row| row == &json!({"resource":MISSING_FIELD_RESOURCE,"problem":format!("vault item {LOGIN_ITEM} has no totp_secret field")})), format!("broken verify omitted {MISSING_FIELD_RESOURCE}: {broken_report}"))?;
    fixture::ensure(broken.iter().any(|row| row == &json!({"resource":MISSING_ITEM_RESOURCE,"problem":"no vault item absent-login-item"})), format!("broken verify omitted {MISSING_ITEM_RESOURCE}: {broken_report}"))?;
    fixture::ensure(
        broken_verify
            .output
            .contains("2 of 4 capability routes do not resolve"),
        format!(
            "routes verify did not summarise the broken routes:\n{}",
            tail(&broken_verify.output, 2000)
        ),
    )?;
    Ok(broken_report)
}
