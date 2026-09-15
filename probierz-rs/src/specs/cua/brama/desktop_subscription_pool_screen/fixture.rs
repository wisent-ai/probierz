//! The real ledger this journey reads, the CLI wrapper that serves it,
//! and the check that no credential material ever reaches the screen.
//!
//! The wrapper is the actual Brama runtime with the ledger bound to it,
//! plus two behaviours the journey depends on: it records every
//! invocation, and it refuses `subscription refresh` outright, because
//! this journey must never refresh a pooled subscription.

use super::*;

/// Executable bits for the wrapper and the stub router.
const EXECUTABLE_MODE: u32 = 0o755;

/// Exit code the wrapper answers a refresh with, so an accidental
/// refresh is loud rather than silent.
const REFRESH_REFUSED_EXIT: u8 = 3;

/// Milliseconds in a day, for the ledger's expiry timestamps.
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// How long ago the ledger's recorded states were written.
const RECORDED_AGO_MS: i64 = 3_600_000;

/// How far the live grant is from expiring, and how long ago the
/// expired one lapsed.
const LIVE_EXPIRY_DAYS: i64 = 120;
const EXPIRED_DAYS_AGO: i64 = 9;

pub(crate) struct Fixture {
    pub(crate) scratch: PathBuf,
    pub(crate) wrapper: PathBuf,
    pub(crate) ledger: PathBuf,
    pub(crate) router: PathBuf,
    pub(crate) invocations: PathBuf,
    pub(crate) runtime: PathBuf,
}

impl Fixture {
    pub(crate) fn new(executable: &std::path::Path) -> Result<Self, String> {
        let home = std::env::var("HOME")
            .map_err(|_| "HOME is required to build the Brama subscription fixture".to_string())?;
        let scratch =
            PathBuf::from(home).join("Library/Caches/probierz-vg-journeys/brama-subscription-pool");
        Ok(Self {
            wrapper: scratch.join("brama"),
            ledger: scratch.join("subscription-usage.json"),
            router: scratch.join("entitlements-router"),
            invocations: scratch.join("invocations.log"),
            runtime: executable
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("brama-runtime"),
            scratch,
        })
    }

    pub(crate) fn build(&self) -> Result<(), String> {
        if !self.runtime.is_file() {
            return Err(format!(
                "the bundled brama runtime is required at {}",
                self.runtime.display()
            ));
        }
        let _ = fs::remove_dir_all(&self.scratch);
        fs::create_dir_all(&self.scratch)
            .map_err(|error| format!("{}: {error}", self.scratch.display()))?;

        fs::write(
            &self.ledger,
            format!("{}\n", serde_json::to_string_pretty(&self.ledger_document()).unwrap()),
        )
        .map_err(|error| format!("{}: {error}", self.ledger.display()))?;

        // A router that always fails, so nothing in this journey can
        // reach a real entitlements service.
        fs::write(&self.router, "#!/bin/sh\nexit 1\n").map_err(|error| error.to_string())?;
        fs::set_permissions(&self.router, fs::Permissions::from_mode(EXECUTABLE_MODE))
            .map_err(|error| error.to_string())?;

        fs::write(&self.wrapper, self.wrapper_script())
            .map_err(|error| error.to_string())?;
        fs::set_permissions(&self.wrapper, fs::Permissions::from_mode(EXECUTABLE_MODE))
            .map_err(|error| error.to_string())?;
        fs::write(&self.invocations, "").map_err(|error| error.to_string())?;
        Ok(())
    }

    /// The pool the screen must render: one live grant, one the
    /// provider refused, one that expired, and one the ledger says
    /// nothing about.
    fn ledger_document(&self) -> Value {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        json!({
            "subscriptions": {
                "sub-anthropic-7f21": {
                    "provider": "anthropic",
                    "credential": {
                        "state": "active",
                        "recorded_at_ms": now - RECORDED_AGO_MS,
                        "expires_at_ms": now + LIVE_EXPIRY_DAYS * DAY_MS
                    },
                    "credential_value": "sk-probierz-NEVERRENDER-anthropic"
                },
                "sub-openai-1c04": {
                    "provider": "openai",
                    "credential": {
                        "state": "needs_reauthorization",
                        "cause": "the provider refused the stored grant: seat revoked",
                        "recorded_at_ms": now - RECORDED_AGO_MS
                    },
                    "credential_value": "sk-probierz-NEVERRENDER-openai"
                },
                "sub-google-93bd": {
                    "provider": "google",
                    "credential": {
                        "state": "active",
                        "recorded_at_ms": now - RECORDED_AGO_MS,
                        "expires_at_ms": now - EXPIRED_DAYS_AGO * DAY_MS
                    },
                    "probe": {
                        "attempted_at_ms": now - RECORDED_AGO_MS,
                        "ok": false,
                        "detail": "the pooled grant expired before the last dispatch"
                    },
                    "credential_value": "sk-probierz-NEVERRENDER-google"
                },
                "sub-mistral-4a88": {
                    "provider": "mistral",
                    "credential_value": "sk-probierz-NEVERRENDER-mistral"
                }
            }
        })
    }

    /// The wrapper: record the invocation, refuse a refresh, otherwise
    /// run the real runtime with the fixture's ledger and router.
    fn wrapper_script(&self) -> String {
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {invocations}\n\
             if [ \"$1\" = \"subscription\" ] && [ \"$2\" = \"refresh\" ]; then\n  \
             printf 'error: this probierz journey never refreshes a pooled subscription\\n' >&2\n  \
             exit {refused}\nfi\n\
             BRAMA_SUBSCRIPTION_USAGE_FILE={ledger} \\\n\
             ENTITLEMENTS_ROUTER_BIN={router} \\\n\
             exec {runtime} \"$@\"\n",
            invocations = shell_quote(&self.invocations),
            refused = REFRESH_REFUSED_EXIT,
            ledger = shell_quote(&self.ledger),
            router = shell_quote(&self.router),
            runtime = shell_quote(&self.runtime)
        )
    }

    /// Every CLI invocation the application made, in order.
    pub(crate) fn invocations(&self) -> Result<Vec<String>, String> {
        Ok(fs::read_to_string(&self.invocations)
            .map_err(|error| format!("{}: {error}", self.invocations.display()))?
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect())
    }

    /// The environment the application is launched with: the wrapper as
    /// its CLI, and the fixture's ledger and router.
    pub(crate) fn environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "BRAMA_BIN".to_string(),
                self.wrapper.to_string_lossy().into_owned(),
            ),
            (
                "BRAMA_SUBSCRIPTION_USAGE_FILE".to_string(),
                self.ledger.to_string_lossy().into_owned(),
            ),
            (
                "ENTITLEMENTS_ROUTER_BIN".to_string(),
                self.router.to_string_lossy().into_owned(),
            ),
        ])
    }
}

pub(crate) fn shell_quote(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

/// The screen must render no credential material at all: not the
/// ledger's decoy values, and nothing else shaped like a credential.
pub(crate) fn assert_no_secret(tree: &str, where_: &str) -> Result<(), String> {
    for (name, pattern) in [
        ("the ledger decoy", r"NEVERRENDER"),
        ("an API-key-shaped string", r"\bsk-[A-Za-z0-9_-]{6,}"),
        ("a bearer token", r"Bearer\s+[A-Za-z0-9._-]{8,}"),
        ("a JWT", r"\bey[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}"),
    ] {
        if Regex::new(pattern).unwrap().is_match(tree) {
            return Err(format!(
                "{where_} must render no credential material ({name})"
            ));
        }
    }
    Ok(())
}
