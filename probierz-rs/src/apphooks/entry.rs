use serde_json::json;
use crate::apphooks::*;
pub(crate) const OKO_REQUIRED: [&str; 5] = [
    "PROBIERZ_RUN_ID",
    "OKO_E2E_EMAIL",
    "OKO_E2E_SUPABASE_URL",
    "OKO_E2E_SUPABASE_ANON_KEY",
    "OKO_E2E_SUPABASE_SERVICE_ROLE_KEY",
];
pub(crate) const OKO_SLACK_REQUIRED: [&str; 3] = [
    "OKO_E2E_SLACK_BOT_TOKEN",
    "OKO_E2E_SLACK_USER_TOKEN",
    "OKO_E2E_SLACK_CHANNEL",
];
pub(crate) const OKO_ACCOUNT_REQUIRED: [&str; 3] = [
    "OKO_E2E_EMAIL",
    "OKO_E2E_SUPABASE_URL",
    "OKO_E2E_SUPABASE_SERVICE_ROLE_KEY",
];
pub(crate) const OKO_BROKER_REQUIRED: [&str; 3] = [
    "OKO_E2E_EMAIL",
    "OKO_E2E_OTP_BROKER_URL",
    "OKO_E2E_OTP_BROKER_TOKEN",
];
pub(crate) const ORG_PREFIX: &str = "probierz-oko-e2e-";

/// What an application manifest may name, and what each capability accepts.
///
/// A manifest declares a capability by name, so an operator has to be able to
/// read the same list the dispatcher matches on. `apphook --help` prints this.
#[macro_export]
macro_rules! apphook_help {
    () => {
        "\
Capabilities (named by an application manifest):
  oko.seed                            Create the isolated organization, author
                                      and fixture state one Oko journey needs
  oko.cleanup                         Remove the organization and account that
                                      seeding created
  oko.ensure-technical-account        Create or confirm the technical account
                                      the journey signs in as
  oko.wait-for-otp [OPTIONS]          Wait for the next one-time code and print
                                      JSON containing it for the journey
    --after <ISO>                     Ignore codes delivered before this instant
    --timeout-ms <MS>                 Give up after this long (default 90000)
  oko.writer-update                   Apply the writer update the journey expects
  oko.apply-feedback                  Apply the editorial feedback fixture
  oko.verify-fixture                  Confirm the seeded state is intact
  game-asset-creator.fixtures         Materialize the asset fixtures the suite reads
  game-asset-creator.visual-eval [OPTIONS]
                                      Score rendered assets against the rubric
                                      through the authenticated model router
    --models <DIR>                    Directory containing the GLBs to grade
    --out <DIR>                       Directory that receives renders and report
    --config <FILE>                   Pipeline configuration to read
    --rubric <NAME>                   Built-in rubric name or literal rubric
    --threshold <N>                   Score below which the evaluation fails

Environment:
  GAC_ROOT                            game_asset_creator repository; defaults to
                                      its root in the application manifest
  GAC_FIXTURE_DIR                     Fixture directory; defaults inside the
                                      current run artifacts or harness"
    };
}

pub fn supports(name: &str) -> bool {
    matches!(
        name,
        "oko.seed"
            | "oko.cleanup"
            | "oko.ensure-technical-account"
            | "oko.wait-for-otp"
            | "oko.writer-update"
            | "oko.apply-feedback"
            | "oko.verify-fixture"
            | "game-asset-creator.fixtures"
            | "game-asset-creator.visual-eval"
    )
}

pub fn execute(
    harness: &Path,
    capability: &str,
    args: &[String],
    environment: &BTreeMap<String, String>,
) -> Result<Value, Failure> {
    match capability {
        "oko.seed" => oko_seed(environment),
        "oko.cleanup" => oko_cleanup(environment),
        "oko.ensure-technical-account" => ensure_technical_account(environment),
        "oko.wait-for-otp" => {
            let (after, timeout) = otp_options(args)?;
            oko_wait_for_otp(environment, after, timeout).map(|code| json!({ "code": code }))
        }
        "oko.writer-update" => oko_writer_update(environment),
        "oko.apply-feedback" => oko_apply_feedback(environment),
        "oko.verify-fixture" => oko_verify_fixture(environment),
        "game-asset-creator.fixtures" => gac_fixtures(harness, environment),
        "game-asset-creator.visual-eval" => gac_visual_eval(harness, args, environment),
        other => Err(Failure::invalid(
            "apphook",
            format!("unknown application capability: {other}"),
        )),
    }
}

pub fn command(harness: &Path, capability: &str, args: &[String]) -> Answer {
    let environment = std::env::vars().collect();
    let result = execute(harness, capability, args, &environment)?;
    print_json(&result)
}

pub(crate) fn required<'a>(
    source: &'a BTreeMap<String, String>,
    names: &[&str],
    message: &str,
) -> Result<&'a BTreeMap<String, String>, Failure> {
    let missing: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| source.get(*name).is_none_or(String::is_empty))
        .collect();
    if missing.is_empty() {
        Ok(source)
    } else {
        Err(Failure::config(
            "apphook.environment",
            format!("{message}: {}", missing.join(", ")),
        ))
    }
}

pub(crate) fn selected_journeys(source: &BTreeMap<String, String>) -> BTreeSet<&str> {
    source
        .get("PROBIERZ_JOURNEYS")
        .map(String::as_str)
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect()
}

pub(crate) fn requires_slack(source: &BTreeMap<String, String>) -> bool {
    let journeys = selected_journeys(source);
    journeys.is_empty() || journeys.contains("slack-feedback")
}

pub(crate) fn requires_oko_fixture(source: &BTreeMap<String, String>) -> bool {
    let journeys = selected_journeys(source);
    journeys.is_empty()
        || journeys
            .iter()
            .any(|journey| *journey != "autonomy-experimental")
}

pub(crate) fn required_oko(source: &BTreeMap<String, String>, include_slack: bool) -> Result<(), Failure> {
    let mut names = OKO_REQUIRED.to_vec();
    if include_slack {
        names.extend(OKO_SLACK_REQUIRED);
    }
    required(source, &names, "missing Oko seed configuration").map(|_| ())
}

pub(crate) fn hash12(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))[..12].to_string()
}

pub(crate) fn deterministic_uuid(parts: &[&str]) -> String {
    let mut digest = Sha256::digest(parts.join(":").as_bytes());
    digest[6] = (digest[6] & 0x0f) | 0x40;
    digest[8] = (digest[8] & 0x3f) | 0x80;
    let value = hex::encode(&digest[..16]);
    format!(
        "{}-{}-{}-{}-{}",
        &value[..8],
        &value[8..12],
        &value[12..16],
        &value[16..20],
        &value[20..]
    )
}

pub(crate) fn fixture_slug(run_id: &str, kind: &str, index: Option<usize>) -> String {
    match index {
        Some(index) => format!("e2e-{}-{kind}-{}", hash12(run_id), index + 1),
        None => format!("e2e-{}-{kind}", hash12(run_id)),
    }
}

