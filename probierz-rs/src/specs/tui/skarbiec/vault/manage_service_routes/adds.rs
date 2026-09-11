//! Adding routes: an absent table is refused, the first add creates the table
//! and both journals, an add without a reason changes nothing, the second add
//! retains the previous table as a backup, and repeating it mutates nothing.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

use crate::specs::tui::skarbiec::fixture;

use super::journey::{
    backups_beside, json_lines, ok_json, run_command, tail, Journey, EMAIL_RESOURCE, LOGIN_ITEM,
    PASSWORD_RESOURCE,
};

/// Runs every add of the journey and returns the path of the backup the
/// second add retained.
pub(super) fn add_routes(journey: &mut Journey<'_>) -> Result<String, String> {
    let absent = run_command(
        &mut journey.shell,
        journey.binary,
        &["routes", "list"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes list",
        &mut journey.executed,
    )?;
    fixture::ensure(absent.status != 0, "routes list accepted an absent table")?;
    fixture::ensure(
        absent.output.contains(&format!(
            "no capability routes table at {}",
            journey.routes_table.display()
        )),
        format!(
            "routes list did not name the absent table:\n{}",
            tail(&absent.output, 2000)
        ),
    )?;
    fixture::ensure(
        backups_beside(journey.temp_dir)?.is_empty(),
        "an absent-table list created a backup",
    )?;

    let first_reason =
        "weles was refused at the cloudflare dashboard login: no route for this origin";
    let first_add = ok_json(
        &mut journey.shell,
        journey.binary,
        &[
            "routes",
            "add",
            "--resource",
            EMAIL_RESOURCE,
            "--item",
            LOGIN_ITEM,
            "--field",
            "username",
            "--reason",
            first_reason,
        ],
        &[],
        Duration::from_secs(60),
        "skarbiec routes add email",
        &mut journey.executed,
    )?;
    fixture::ensure(
        first_add
            == json!({"added":true,"resource":EMAIL_RESOURCE,"item":LOGIN_ITEM,"field":"username","backup":null}),
        format!("first routes add answer is wrong: {first_add}"),
    )?;
    let table_after_first_add = fs::read(&journey.routes_table)
        .map_err(|error| format!("{}: {error}", journey.routes_table.display()))?;
    let parsed_table: Value =
        serde_json::from_slice(&table_after_first_add).map_err(|error| error.to_string())?;
    fixture::ensure(
        parsed_table == json!({EMAIL_RESOURCE:{"item":LOGIN_ITEM,"field":"username"}}),
        format!("first routes table is wrong: {parsed_table}"),
    )?;
    let beside_after_first = json_lines(&journey.beside_journal)?;
    fixture::ensure(
        beside_after_first.len() == 1,
        format!(
            "beside journal has {} lines after first add",
            beside_after_first.len()
        ),
    )?;
    fixture::ensure(
        beside_after_first[0]["reason"] == first_reason
            && beside_after_first[0]["resource"] == EMAIL_RESOURCE,
        format!(
            "beside journal first line is wrong: {}",
            beside_after_first[0]
        ),
    )?;
    let chained_after_first = json_lines(&journey.routes_audit_file)?;
    fixture::ensure(
        chained_after_first.len() == 1,
        format!(
            "routes audit has {} lines after first add",
            chained_after_first.len()
        ),
    )?;
    fixture::ensure(
        chained_after_first[0]["op"] == "capability-route-added"
            && chained_after_first[0]["extra"]["reason"] == first_reason,
        format!(
            "routes audit first line is wrong: {}",
            chained_after_first[0]
        ),
    )?;

    let reasonless = run_command(
        &mut journey.shell,
        journey.binary,
        &[
            "routes",
            "add",
            "--resource",
            PASSWORD_RESOURCE,
            "--item",
            LOGIN_ITEM,
            "--field",
            "password",
        ],
        &[],
        Duration::from_secs(60),
        "skarbiec routes add without reason",
        &mut journey.executed,
    )?;
    fixture::ensure(
        reasonless.status != 0,
        "routes add without --reason was accepted",
    )?;
    fixture::ensure(
        reasonless
            .output
            .contains("routes add requires an exact --reason"),
        format!(
            "routes add without --reason did not report the missing reason:\n{}",
            tail(&reasonless.output, 2000)
        ),
    )?;
    fixture::ensure(
        fs::read(&journey.routes_table).map_err(|error| error.to_string())?
            == table_after_first_add,
        "a refused routes add still rewrote the table",
    )?;
    fixture::ensure(
        json_lines(&journey.beside_journal)? == beside_after_first,
        "a refused routes add still rewrote the beside journal",
    )?;
    fixture::ensure(
        json_lines(&journey.routes_audit_file)? == chained_after_first,
        "a refused routes add still rewrote the chained journal",
    )?;
    fixture::ensure(
        backups_beside(journey.temp_dir)?.is_empty(),
        "a refused routes add still snapshotted the table",
    )?;

    let second_reason = "same login form, password field";
    let second_add = ok_json(
        &mut journey.shell,
        journey.binary,
        &[
            "routes",
            "add",
            "--resource",
            PASSWORD_RESOURCE,
            "--item",
            LOGIN_ITEM,
            "--field",
            "password",
            "--reason",
            second_reason,
        ],
        &[],
        Duration::from_secs(60),
        "skarbiec routes add password",
        &mut journey.executed,
    )?;
    fixture::ensure(
        second_add["added"] == true
            && second_add["resource"] == PASSWORD_RESOURCE
            && second_add["item"] == LOGIN_ITEM
            && second_add["field"] == "password",
        format!("second routes add answer is wrong: {second_add}"),
    )?;
    let backup = second_add["backup"]
        .as_str()
        .ok_or_else(|| "routes add did not report a retained backup".to_string())?
        .to_string();
    fixture::ensure(
        backup.starts_with(&format!("{}.before-", journey.routes_table.display())),
        format!("retained backup is not beside the routes table: {backup}"),
    )?;
    fixture::ensure(
        fs::read(&backup).map_err(|error| format!("{backup}: {error}"))? == table_after_first_add,
        "the retained backup does not hold the table as it stood before the add",
    )?;
    let table_after_second_add =
        fs::read(&journey.routes_table).map_err(|error| error.to_string())?;
    let parsed_second: Value =
        serde_json::from_slice(&table_after_second_add).map_err(|error| error.to_string())?;
    fixture::ensure(
        parsed_second
            == json!({EMAIL_RESOURCE:{"item":LOGIN_ITEM,"field":"username"},PASSWORD_RESOURCE:{"item":LOGIN_ITEM,"field":"password"}}),
        format!("second routes table is wrong: {parsed_second}"),
    )?;
    let repeated = ok_json(
        &mut journey.shell,
        journey.binary,
        &[
            "routes",
            "add",
            "--resource",
            PASSWORD_RESOURCE,
            "--item",
            LOGIN_ITEM,
            "--field",
            "password",
            "--reason",
            "provisioning sequence ran again",
        ],
        &[],
        Duration::from_secs(60),
        "skarbiec routes add password repeated",
        &mut journey.executed,
    )?;
    fixture::ensure(
        repeated
            == json!({"added":false,"resource":PASSWORD_RESOURCE,"item":LOGIN_ITEM,"field":"password","backup":null}),
        format!("repeated routes add answer is wrong: {repeated}"),
    )?;
    fixture::ensure(
        fs::read(&journey.routes_table).map_err(|error| error.to_string())?
            == table_after_second_add,
        "a repeated routes add rewrote the table",
    )?;
    fixture::ensure(
        json_lines(&journey.beside_journal)?.len() == 2,
        "a repeated routes add recorded a mutation",
    )?;
    let backup_name = Path::new(&backup)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    fixture::ensure(
        backups_beside(journey.temp_dir)? == vec![backup_name.to_string()],
        format!(
            "unexpected routes backups: {:?}",
            backups_beside(journey.temp_dir)?
        ),
    )?;
    Ok(backup)
}
