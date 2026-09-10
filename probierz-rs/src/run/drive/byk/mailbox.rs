use serde_json::json;
use crate::run::*;

/// Where the mailbox broker executable comes from.
///
/// A harness does not build another repository. This used to `cargo build
/// --bin skarbiec-entitlements-router` inside `entitlements-rotator`, which
/// stopped existing on 2026-07-28 when that repository removed its vendored
/// copy of the vault (commit 525f7d6, "Stop being a second source and
/// publisher of Skarbiec"). The journey kept building a binary nobody
/// produced any more and reported it as a build failure, which hid what had
/// actually happened.
///
/// So the broker is now what it always was in truth: an operator-provisioned
/// executable. `BYK_MAILBOX_BROKER` names it, and the refusal says what it
/// must be able to do.
pub(crate) fn byk_broker_binary(
    _harness: &Path,
    env: &BTreeMap<String, String>,
    _timeout_ms: u64,
) -> Result<(PathBuf, PathBuf, BTreeMap<String, String>), String> {
    // The operator's shell counts: a `KEY=VALUE` argument wins, and an
    // exported variable is honoured, exactly as every other condition is.
    let broker_env = byk_broker_environment(&env_snapshot(env));
    let declared = broker_env
        .get("BYK_MAILBOX_BROKER")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!(
            "BYK_MAILBOX_BROKER is required: an executable serving `mailbox-broker --mailbox {BYK_MAILBOX} --socket <path>`, \
`mailbox-probe --mailbox {BYK_MAILBOX}` and `seed-resend <env-file>`. \
No installed product provides it: entitlements-rotator removed its vendored vault binary in 525f7d6 on 2026-07-28 and the surviving copy is the vendored-superset branch of wisent-ai/skarbiec"
        ))?;
    let broker = PathBuf::from(&declared);
    if !broker.is_absolute() {
        return Err(format!(
            "BYK_MAILBOX_BROKER must be an absolute path, not {declared}"
        ));
    }
    let metadata = fs::metadata(&broker)
        .map_err(|error| format!("BYK_MAILBOX_BROKER {declared} cannot be read: {error}"))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!(
            "BYK_MAILBOX_BROKER {declared} is not an executable file"
        ));
    }
    let working = broker
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));
    Ok((broker, working, broker_env))
}

/// Is the login mailbox reachable from here? This is the readiness question
/// `check` asks, and it is the only one that cannot be answered by looking at
/// a file: the broker has to open the mailbox and say so.
pub(crate) fn byk_mailbox_reachable(harness: &Path, env: &BTreeMap<String, String>) -> (bool, String) {
    let (broker, rotator, broker_env) = match byk_broker_binary(harness, env, DEFAULT_TIMEOUT_MS) {
        Ok(parts) => parts,
        Err(reason) => return (false, reason),
    };
    let probe = capture(
        broker.to_string_lossy().as_ref(),
        &[
            "mailbox-probe".into(),
            "--mailbox".into(),
            BYK_MAILBOX.into(),
        ],
        Some(&rotator),
        Some(&broker_env),
        Some(DEFAULT_TIMEOUT_MS),
    );
    if probe.status.is_some_and(|status| status.success()) {
        return (true, String::new());
    }
    let detail = tail_chars(&String::from_utf8_lossy(&probe.stderr), 400)
        .trim()
        .to_string();
    (
        false,
        if detail.is_empty() {
            format!("the {BYK_MAILBOX} mailbox did not answer")
        } else {
            format!("the {BYK_MAILBOX} mailbox did not answer: {detail}")
        },
    )
}

/// Seed the mailbox's resend source and stop. The journey needs an address a
/// resend can come from; seeding it is an operator action on real mail state,
/// so it is its own mode and never a side effect of running the journey.
pub(crate) fn seed_byk_resend(harness: &Path, env: &BTreeMap<String, String>) -> Answer {
    let (broker, rotator, broker_env) = byk_broker_binary(harness, env, DEFAULT_TIMEOUT_MS)
        .map_err(|reason| fail("run.byk.seed", reason))?;
    let source = harness
        .parent()
        .unwrap_or(harness)
        .join("weles")
        .join(".env");
    if !source.exists() {
        return Err(fail(
            "run.byk.seed",
            format!("the resend source {} does not exist", source.display()),
        ));
    }
    let status = Command::new(&broker)
        .args(["seed-resend", source.to_string_lossy().as_ref()])
        .current_dir(&rotator)
        .envs(&broker_env)
        .stdin(Stdio::null())
        .status()
        .map_err(|error| {
            fail(
                "run.byk.seed",
                format!("could not start the Skarbiec mailbox broker: {error}"),
            )
        })?;
    if !status.success() {
        return Err(fail(
            "run.byk.seed",
            format!(
                "seeding the {BYK_MAILBOX} resend source failed with exit {}",
                status.code().unwrap_or(-1)
            ),
        ));
    }
    print_json(&json!({
        "target": "mobile:ios:byk-auth",
        "action": "seed-resend",
        "mailbox": BYK_MAILBOX,
        "source": source,
        "seeded": true,
    }))
}

