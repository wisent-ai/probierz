use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use regex::Regex;
use serde_json::{json, Value};

use crate::specs;

use super::skarbiec_fixture::{self as fixture, CommandResult, Shell};

const EMAIL_RESOURCE: &str = "origin:https://dash.cloudflare.com/email";
const PASSWORD_RESOURCE: &str = "origin:https://dash.cloudflare.com/password";
const MISSING_ITEM_RESOURCE: &str = "provider:probierz-absent-item";
const MISSING_FIELD_RESOURCE: &str = "provider:probierz-absent-field";
const LOGIN_ITEM: &str = "platform-admin-cloudflare";
const SECRET_VALUE: &str = "routes-journey-secret-4b71e0";

pub fn run(context: &specs::Context) -> Result<(), String> {
    let binary = fixture::binary(context);
    let manifest = fs::read_to_string(context.harness.join("apps/skarbiec/probierz.yaml"))
        .map_err(|error| {
            format!("skarbiec manifest must provide the source repository root: {error}")
        })?;
    let source_root = manifest
        .lines()
        .find_map(|line| line.strip_prefix("  - root: "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "skarbiec manifest must provide the source repository root".to_string())?;
    let revision = Command::new("/usr/bin/git")
        .args(["-C", source_root, "rev-parse", "HEAD"])
        .output()
        .map_err(|error| format!("cannot resolve skarbiec source revision: {error}"))?;
    fixture::ensure(
        revision.status.success(),
        format!(
            "cannot resolve skarbiec source revision: {}",
            String::from_utf8_lossy(&revision.stderr)
        ),
    )?;
    let source_revision = String::from_utf8_lossy(&revision.stdout).trim().to_string();
    let sha = Regex::new(r"^[0-9a-f]{40}$").map_err(|error| error.to_string())?;
    fixture::ensure(
        sha.is_match(&source_revision),
        "skarbiec source revision is not a full Git SHA",
    )?;
    let status = Command::new("/usr/bin/git")
        .args(["-C", source_root, "status", "--porcelain"])
        .output()
        .map_err(|error| format!("cannot inspect skarbiec source state: {error}"))?;
    fixture::ensure(
        status.status.success(),
        format!(
            "cannot inspect skarbiec source state: {}",
            String::from_utf8_lossy(&status.stderr)
        ),
    )?;
    let source_dirty = !status.stdout.is_empty();

    let temp_dir = fixture::scratch("skb-routes")?;
    let result = run_fixture(
        context,
        &binary,
        source_root,
        &source_revision,
        source_dirty,
        &temp_dir,
    );
    fixture::clean(&temp_dir);
    result
}

fn run_fixture(
    context: &specs::Context,
    binary: &str,
    source_root: &str,
    source_revision: &str,
    source_dirty: bool,
    temp_dir: &Path,
) -> Result<(), String> {
    let vault_file = temp_dir.join("routes.vault.json");
    let routes_table = temp_dir.join("capability-routes.json");
    let beside_journal = temp_dir.join("capability-routes.audit.jsonl");
    let routes_audit_file = temp_dir.join("routes.audit.jsonl");
    let setup_audit_file = temp_dir.join("fixture-setup.audit.jsonl");
    let env = fixture::env(&[
        ("GNUPGHOME", temp_dir),
        ("SKARBIEC_VAULT_FILE", &vault_file),
        ("SKARBIEC_AUDIT_FILE", &routes_audit_file),
        ("SKARBIEC_CAPABILITY_ROUTES_FILE", &routes_table),
    ]);
    let mut shell = Shell::spawn(
        "__SKARBIEC_ROUTES_READY__",
        "__SKARBIEC_ROUTES_",
        None,
        &env,
        120,
        36,
    )?;
    let mut executed = Vec::new();

    let help = ok_json(
        &mut shell,
        binary,
        &["routes", "help"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes help",
        &mut executed,
    )?;
    fixture::ensure(
        help["table"].as_str() == Some(routes_table.to_string_lossy().as_ref()),
        format!("routes help resolved the wrong table: {help}"),
    )?;
    fixture::ensure(
        help["commands"]
            == json!([
                "routes list [<consumer>]",
                "routes add --resource <resource> --item <item> --field <field> --reason <text>",
                "routes verify [<consumer>]"
            ]),
        format!("routes help returned the wrong commands: {help}"),
    )?;
    let setup_path = setup_audit_file.to_string_lossy().into_owned();
    let initialized = ok_json(
        &mut shell,
        binary,
        &["init", "routes-journey-owner"],
        &[("SKARBIEC_AUDIT_FILE", setup_path.as_str())],
        Duration::from_secs(180),
        "skarbiec init routes-journey-owner",
        &mut executed,
    )?;
    fixture::ensure(
        initialized["ok"] == true,
        "vault initialization did not report ok",
    )?;
    fixture::ensure(
        initialized["vault"].as_str() == Some(vault_file.to_string_lossy().as_ref()),
        format!("initialized vault path is not {}", vault_file.display()),
    )?;
    let secret_field = format!("password={SECRET_VALUE}");
    let stored = ok_json(
        &mut shell,
        binary,
        &[
            "set",
            LOGIN_ITEM,
            "--type",
            "login",
            "username=ops@cloudflare.invalid",
            &secret_field,
        ],
        &[("SKARBIEC_AUDIT_FILE", setup_path.as_str())],
        Duration::from_secs(60),
        &format!("skarbiec set {LOGIN_ITEM} (redacted)"),
        &mut executed,
    )?;
    fixture::ensure(
        stored == json!({"id":LOGIN_ITEM,"kind":"login","ok":true}),
        format!("stored answer is wrong: {stored}"),
    )?;
    let routes_phase_start = shell.full_log().len();

    let absent = run_command(
        &mut shell,
        binary,
        &["routes", "list"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes list",
        &mut executed,
    )?;
    fixture::ensure(absent.status != 0, "routes list accepted an absent table")?;
    fixture::ensure(
        absent.output.contains(&format!(
            "no capability routes table at {}",
            routes_table.display()
        )),
        format!(
            "routes list did not name the absent table:\n{}",
            tail(&absent.output, 2000)
        ),
    )?;
    fixture::ensure(
        backups_beside(temp_dir)?.is_empty(),
        "an absent-table list created a backup",
    )?;

    let first_reason =
        "weles was refused at the cloudflare dashboard login: no route for this origin";
    let first_add = ok_json(
        &mut shell,
        binary,
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
        &mut executed,
    )?;
    fixture::ensure(
        first_add
            == json!({"added":true,"resource":EMAIL_RESOURCE,"item":LOGIN_ITEM,"field":"username","backup":null}),
        format!("first routes add answer is wrong: {first_add}"),
    )?;
    let table_after_first_add =
        fs::read(&routes_table).map_err(|error| format!("{}: {error}", routes_table.display()))?;
    let parsed_table: Value =
        serde_json::from_slice(&table_after_first_add).map_err(|error| error.to_string())?;
    fixture::ensure(
        parsed_table == json!({EMAIL_RESOURCE:{"item":LOGIN_ITEM,"field":"username"}}),
        format!("first routes table is wrong: {parsed_table}"),
    )?;
    let beside_after_first = json_lines(&beside_journal)?;
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
    let chained_after_first = json_lines(&routes_audit_file)?;
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
        &mut shell,
        binary,
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
        &mut executed,
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
        fs::read(&routes_table).map_err(|error| error.to_string())? == table_after_first_add,
        "a refused routes add still rewrote the table",
    )?;
    fixture::ensure(
        json_lines(&beside_journal)? == beside_after_first,
        "a refused routes add still rewrote the beside journal",
    )?;
    fixture::ensure(
        json_lines(&routes_audit_file)? == chained_after_first,
        "a refused routes add still rewrote the chained journal",
    )?;
    fixture::ensure(
        backups_beside(temp_dir)?.is_empty(),
        "a refused routes add still snapshotted the table",
    )?;

    let second_reason = "same login form, password field";
    let second_add = ok_json(
        &mut shell,
        binary,
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
        &mut executed,
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
        backup.starts_with(&format!("{}.before-", routes_table.display())),
        format!("retained backup is not beside the routes table: {backup}"),
    )?;
    fixture::ensure(
        fs::read(&backup).map_err(|error| format!("{backup}: {error}"))? == table_after_first_add,
        "the retained backup does not hold the table as it stood before the add",
    )?;
    let table_after_second_add = fs::read(&routes_table).map_err(|error| error.to_string())?;
    let parsed_second: Value =
        serde_json::from_slice(&table_after_second_add).map_err(|error| error.to_string())?;
    fixture::ensure(
        parsed_second
            == json!({EMAIL_RESOURCE:{"item":LOGIN_ITEM,"field":"username"},PASSWORD_RESOURCE:{"item":LOGIN_ITEM,"field":"password"}}),
        format!("second routes table is wrong: {parsed_second}"),
    )?;
    let repeated = ok_json(
        &mut shell,
        binary,
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
        &mut executed,
    )?;
    fixture::ensure(
        repeated
            == json!({"added":false,"resource":PASSWORD_RESOURCE,"item":LOGIN_ITEM,"field":"password","backup":null}),
        format!("repeated routes add answer is wrong: {repeated}"),
    )?;
    fixture::ensure(
        fs::read(&routes_table).map_err(|error| error.to_string())? == table_after_second_add,
        "a repeated routes add rewrote the table",
    )?;
    fixture::ensure(
        json_lines(&beside_journal)?.len() == 2,
        "a repeated routes add recorded a mutation",
    )?;
    let backup_name = Path::new(&backup)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    fixture::ensure(
        backups_beside(temp_dir)? == vec![backup_name.to_string()],
        format!("unexpected routes backups: {:?}", backups_beside(temp_dir)?),
    )?;

    let sound_list = ok_json(
        &mut shell,
        binary,
        &["routes", "list"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes list",
        &mut executed,
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
        &mut shell,
        binary,
        &["routes", "list", "dash.cloudflare.com/password"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes list dash.cloudflare.com/password",
        &mut executed,
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
        &mut shell,
        binary,
        &["routes", "verify"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes verify",
        &mut executed,
    )?;
    fixture::ensure(
        sound_verify == json!({"checked":2,"broken":[]}),
        format!("sound verify answer is wrong: {sound_verify}"),
    )?;

    let broken_item = ok_json(
        &mut shell,
        binary,
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
        &mut executed,
    )?;
    fixture::ensure(
        broken_item["added"] == true,
        format!("broken item route was not added: {broken_item}"),
    )?;
    let broken_field = ok_json(
        &mut shell,
        binary,
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
        &mut executed,
    )?;
    fixture::ensure(
        broken_field["added"] == true,
        format!("broken field route was not added: {broken_field}"),
    )?;
    let broken_list = ok_json(
        &mut shell,
        binary,
        &["routes", "list"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes list",
        &mut executed,
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
        &mut shell,
        binary,
        &["routes", "verify"],
        &[],
        Duration::from_secs(60),
        "skarbiec routes verify",
        &mut executed,
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

    let phase_log = shell.full_log();
    let phase_log = phase_log.get(routes_phase_start..).unwrap_or(&phase_log);
    fixture::ensure(
        !phase_log.contains(SECRET_VALUE),
        "a capability routes command emitted the secret its route points at",
    )?;
    for path in [&routes_table, &beside_journal, &routes_audit_file] {
        let contents =
            fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        fixture::ensure(
            !contents.contains(SECRET_VALUE),
            format!("{} carries secret material", path.display()),
        )?;
    }

    fixture::write_trace(
        context,
        "skarbiec-manage-service-routes.trace.json",
        json!({
            "schemaVersion": 1,
            "kind": "probierz-skarbiec-manage-service-routes-trace",
            "journey": "manage-service-routes",
            "runId": context.optional("PROBIERZ_RUN_ID"),
            "status": "completed",
            "observation": {
                "sourceRoot": source_root,
                "sourceRevision": source_revision,
                "sourceDirty": source_dirty,
                "binary": binary,
                "routesTable": routes_table,
                "commands": executed,
                "soundVerify": sound_verify,
                "brokenVerify": broken_report,
                "retainedBackup": backup,
            },
            "contracts": [
                "routes add without --reason exits non-zero and leaves the table, both journals, and the backup series untouched",
                "routes add with --reason reports the added resource, item, and field and retains the previous table as the backup path it names",
                "a second routes add leaves the existing route untouched, and repeating one reports added=false with no backup and no mutation",
                "routes list reports every route with its item, its field, and whether the vault holds that item and that field",
                "routes verify exits zero with no broken entries on a sound table",
                "routes verify exits non-zero on a broken table and names each broken resource and its problem on stdout",
                "no capability routes command emits the credential its routes point at"
            ],
            "redaction": {"status":"verified_redacted","credentialsIncluded":false,"privateRecordsIncluded":false},
            "publicationRequirements": {"artifactKind":"trace","minimumEvidence":"E2","redactionStatus":"verified_redacted"}
        }),
    )?;
    shell.close()
}

fn run_command(
    shell: &mut Shell,
    binary: &str,
    args: &[&str],
    env: &[(&str, &str)],
    timeout: Duration,
    label: &str,
    executed: &mut Vec<Value>,
) -> Result<CommandResult, String> {
    let result = shell.run_program(binary, args, env, timeout)?;
    executed.push(json!({"command":label,"exitStatus":result.status}));
    Ok(result)
}

fn ok_json(
    shell: &mut Shell,
    binary: &str,
    args: &[&str],
    env: &[(&str, &str)],
    timeout: Duration,
    label: &str,
    executed: &mut Vec<Value>,
) -> Result<Value, String> {
    let result = run_command(shell, binary, args, env, timeout, label, executed)?;
    fixture::ensure(
        result.status == 0,
        format!(
            "skarbiec {} exited {}:\n{}",
            args.join(" "),
            result.status,
            tail(&result.output, 2000)
        ),
    )?;
    fixture::parse_json(&result.output, || {
        format!("skarbiec {} emitted no JSON", args.join(" "))
    })
}

fn json_lines(path: &Path) -> Result<Vec<Value>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|error| format!("{}: {error}", path.display()))
        })
        .collect()
}

fn backups_beside(temp_dir: &Path) -> Result<Vec<String>, String> {
    let mut backups = fs::read_dir(temp_dir)
        .map_err(|error| format!("{}: {error}", temp_dir.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("capability-routes.json.before-"))
        .collect::<Vec<_>>();
    backups.sort();
    Ok(backups)
}

fn route_row<'a>(rows: &'a [Value], resource: &str) -> Result<&'a Value, String> {
    rows.iter()
        .find(|row| row["resource"] == resource)
        .ok_or_else(|| format!("routes list omitted {resource}"))
}

fn tail(text: &str, limit: usize) -> String {
    let mut chars = text.chars().rev().take(limit).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}
