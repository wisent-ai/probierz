//! Where the fixture's documents live, the stub router that serves the
//! listing, and the environment that binds both to the released Brama
//! executable.
//!
//! The router serves `list` from a file and refuses every other verb,
//! so the journey can prove the report used the listing verb only and
//! redeemed no capability.

use super::*;

/// The stub router is owner-executable only.
#[cfg(unix)]
const ROUTER_MODE: u32 = 0o700;

/// Exit code the stub router answers any verb but `list` with.
const ROUTER_REFUSED_EXIT: u8 = 3;

/// Offsets, in milliseconds, that place the fixture's credentials
/// either side of now: one that lapsed half an hour ago, one that
/// lapsed ten minutes ago, one that expires tomorrow, and one within
/// the hour.
const BURNT_AGO_MS: i64 = 1_800_000;
const EXPIRED_AGO_MS: i64 = 600_000;
const LIVE_AHEAD_MS: i64 = 86_400_000;
const SHORT_AHEAD_MS: i64 = 3_600_000;

pub(crate) struct Fixture {
    pub(crate) temp: std::path::PathBuf,
    pub(crate) state: std::path::PathBuf,
    pub(crate) home: std::path::PathBuf,
    pub(crate) ledger_path: std::path::PathBuf,
    pub(crate) router_log: std::path::PathBuf,
    pub(crate) environment: BTreeMap<String, String>,
    pub(crate) facts: LedgerFacts,
}

impl Fixture {
    pub(crate) fn build() -> Result<Self, String> {
        let temp = common::scratch("brama-subscription-pool")?;
        let state = temp.join("state");
        let home = temp.join("home");
        let bin = temp.join("bin");
        for directory in [&state, &home, &bin] {
            fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let facts = LedgerFacts {
            now,
            burnt: now - BURNT_AGO_MS,
            expired: now - EXPIRED_AGO_MS,
            live: now + LIVE_AHEAD_MS,
            short: now + SHORT_AHEAD_MS,
            burnt_cause: "invalid_grant: refresh token is no longer accepted",
            active_reason: "429 from provider: this account is over its plan",
            lapsed_reason: "429 from provider: a lapsed block that must not be reported",
        };

        let vault_path = temp.join("vault-list.json");
        let ledger_path = state.join("subscription-usage.json");
        common::write_json(&vault_path, &vault_listing())?;
        common::write_json(&ledger_path, &ledger(&facts))?;
        fs::write(state.join("journal.jsonl"), "").map_err(|e| e.to_string())?;

        let router = bin.join("entitlements-router");
        let router_log = temp.join("entitlements-router.invocations");
        write_router(&router, &router_log, &vault_path)?;

        Ok(Self {
            environment: environment(&temp, &state, &home, &ledger_path, &router),
            temp,
            state,
            home,
            ledger_path,
            router_log,
            facts,
        })
    }

    /// The state and home trees, as one string, so "wrote nothing" is
    /// a single comparison.
    pub(crate) fn tree_fingerprint(&self) -> Result<String, String> {
        Ok(format!(
            "{}\n--\n{}",
            fingerprint(&self.state)?,
            fingerprint(&self.home)?
        ))
    }

    pub(crate) fn router_invocations(&self) -> String {
        fs::read_to_string(&self.router_log).unwrap_or_default()
    }

    pub(crate) fn remove(&self) {
        common::remove(&self.temp);
    }
}

/// A router that records every verb, serves the listing, and refuses
/// everything else — so no capability can be redeemed here.
fn write_router(router: &Path, log: &Path, listing: &Path) -> Result<(), String> {
    fs::write(
        router,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\n\
             if [ \"$1\" = list ]; then cat '{listing}'; exit 0; fi\n\
             printf 'fixture entitlements router refuses %s: a Probierz fixture redeems no capability\\n' \"$1\" >&2\n\
             exit {refused}\n",
            log = log.display(),
            listing = listing.display(),
            refused = ROUTER_REFUSED_EXIT
        ),
    )
    .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(router, fs::Permissions::from_mode(ROUTER_MODE))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Everything the executable is told, so it reads this fixture and
/// nothing on the host.
fn environment(
    temp: &Path,
    state: &Path,
    home: &Path,
    ledger_path: &Path,
    router: &Path,
) -> BTreeMap<String, String> {
    common::env_map([
        ("HOME", home.to_string_lossy().as_ref()),
        (
            "XDG_STATE_HOME",
            temp.join("xdg-state").to_string_lossy().as_ref(),
        ),
        ("BRAMA_STATE_DIR", state.to_string_lossy().as_ref()),
        (
            "BRAMA_SUBSCRIPTION_USAGE_FILE",
            ledger_path.to_string_lossy().as_ref(),
        ),
        (
            "BRAMA_MODEL_CATALOG_CACHE",
            temp.join("model-catalog.json").to_string_lossy().as_ref(),
        ),
        (
            "BRAMA_PERF_PATH",
            temp.join("perf.json").to_string_lossy().as_ref(),
        ),
        (
            "BRAMA_DONATED_SUBSCRIPTIONS_FILE",
            temp.join("donated-subscriptions.json")
                .to_string_lossy()
                .as_ref(),
        ),
        ("BRAMA_SUBSCRIPTION_CATALOG", "{\"items\":[]}"),
        (
            "SKARBIEC_CAPABILITY_ROUTES_FILE",
            temp.join("capability-routes.json")
                .to_string_lossy()
                .as_ref(),
        ),
        ("ENTITLEMENTS_ROUTER_BIN", router.to_string_lossy().as_ref()),
    ])
}
