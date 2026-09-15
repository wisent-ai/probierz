//! What the evaluation is run against: the mode, the base URL, the
//! declared policy and the approved brief, and the model settings the
//! graders need.
//!
//! Everything here refuses rather than assumes: an unknown mode, a URL
//! carrying credentials, a policy at an unsupported schema version, or
//! a missing router credential stops the evaluation before a single
//! page is fetched.

use super::*;

/// Modes an evaluation may run in. `production` additionally requires
/// Search Console and CrUX evidence.
const MODES: [&str; 4] = ["pull-request", "release", "nightly", "production"];

/// Schema version this evaluation understands for both documents.
const SCHEMA_VERSION: u64 = 1;

/// The application whose pages are evaluated. Every application in
/// this harness declares its SEO contract under the `web` surface.
const SURFACE: &str = "web";

/// The declared contract: the policy, the approved brief, the routes
/// and the base URL they hang off.
pub(crate) struct Contract {
    pub(crate) mode: String,
    pub(crate) canonical: url::Url,
    pub(crate) policy: JsonValue,
    pub(crate) policy_path: PathBuf,
    pub(crate) brief: JsonValue,
    pub(crate) brief_path: PathBuf,
    /// The application manifest's `seo` section, which says whether
    /// this mode requires a signature.
    pub(crate) seo: YamlValue,
    pub(crate) loaded: manifest::Manifest,
}

impl Contract {
    pub(crate) fn load(
        harness: &Path,
        app_id: &str,
        base_url: &str,
        mode: &str,
        policy_file: Option<&Path>,
        brief_file: Option<&Path>,
    ) -> Result<Self, Failure> {
        if !MODES.contains(&mode) {
            return Err(Failure::invalid(
                "seo-evaluate",
                format!(
                    "invalid SEO contract: mode must be one of {}",
                    MODES.join(", ")
                ),
            ));
        }
        let canonical = canonical_base_url(base_url)?;

        let loaded = manifest::load(harness, app_id)?;
        let seo = loaded
            .document
            .get("seo")
            .ok_or_else(|| {
                Failure::config(
                    "seo-evaluate",
                    format!(
                        "invalid SEO contract: {} seo section is required",
                        loaded.file.display()
                    ),
                )
            })?
            .clone();

        let policy_path = resolved_contract_file(
            harness,
            policy_file,
            seo.get("policy").and_then(YamlValue::as_str),
            "SEO policy",
        )?;
        let brief_env = std::env::var("PROBIERZ_LANDING_BRIEF").ok();
        let brief_path = resolved_contract_file(
            harness,
            brief_file,
            brief_env
                .as_deref()
                .or_else(|| seo.get("brief").and_then(YamlValue::as_str)),
            "landing brief",
        )?;

        let policy = read_document(&policy_path, "SEO policy")?;
        let brief = read_document(&brief_path, "landing brief")?;
        if policy.get("schemaVersion").and_then(JsonValue::as_u64) != Some(SCHEMA_VERSION)
            || policy
                .get("dimensions")
                .and_then(JsonValue::as_object)
                .is_none()
        {
            return Err(Failure::config(
                "seo-evaluate",
                format!(
                    "invalid SEO contract: {} schemaVersion must be {SCHEMA_VERSION} and dimensions are required",
                    policy_path.display()
                ),
            ));
        }
        if brief.get("schemaVersion").and_then(JsonValue::as_u64) != Some(SCHEMA_VERSION) {
            return Err(Failure::config(
                "seo-evaluate",
                format!(
                    "invalid SEO contract: {} schemaVersion must be {SCHEMA_VERSION}",
                    brief_path.display()
                ),
            ));
        }

        Ok(Self {
            mode: mode.to_string(),
            canonical,
            policy,
            policy_path,
            brief,
            brief_path,
            seo,
            loaded,
        })
    }

    /// The declared routes. Absent routes are a contract error, not an
    /// empty crawl.
    pub(crate) fn routes(&self) -> Result<&Vec<JsonValue>, Failure> {
        self.policy["routes"].as_array().ok_or_else(|| {
            Failure::config(
                "seo-evaluate",
                format!(
                    "invalid SEO contract: {} routes are required",
                    self.policy_path.display()
                ),
            )
        })
    }

    /// Dimension names graded by a model, in policy order.
    pub(crate) fn model_dimension_names(&self) -> Vec<String> {
        self.policy["dimensions"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, rule)| matches!(rule["source"].as_str(), Some("model" | "hybrid")))
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Whether this mode's profile requires a signed report.
    pub(crate) fn signature_required(&self) -> bool {
        self.seo
            .get("profiles")
            .and_then(|value| value.get(&self.mode))
            .and_then(|value| value.get("requireSignature"))
            .and_then(YamlValue::as_bool)
            .unwrap_or(false)
    }
}

/// The router, the models and the credentials the graders use.
pub(crate) struct ModelSettings {
    pub(crate) primary: String,
    pub(crate) secondary: String,
    pub(crate) router_url: String,
    pub(crate) token: String,
    pub(crate) agent_id: String,
    pub(crate) agent_secret: String,
}

impl ModelSettings {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve(
        harness: &Path,
        app_id: &str,
        contract: &Contract,
        primary: Option<&str>,
        secondary: Option<&str>,
        router_url: Option<&str>,
        agent_id: Option<&str>,
        router_bearer: Option<&str>,
        agent_secret: Option<&str>,
    ) -> Result<Self, Failure> {
        let (primary, secondary, router_url) = seo_prerequisites(
            harness,
            app_id,
            Some(SURFACE),
            primary,
            secondary,
            router_url,
        )
        .map_err(|detail| Failure::config("seo-evaluate", detail))?;

        Ok(Self {
            primary,
            secondary,
            router_url,
            token: contract.setting(router_bearer, "STADO_MODEL_ROUTER_TOKEN")?,
            agent_id: contract.setting(agent_id, "PROBIERZ_MODEL_AGENT_ID")?,
            agent_secret: contract.setting(agent_secret, "PROBIERZ_MODEL_AGENT_SECRET")?,
        })
    }
}

impl Contract {
    /// A required setting: the explicit argument when given, otherwise
    /// the one selected for this application's web surface.
    pub(crate) fn setting(&self, explicit: Option<&str>, name: &str) -> Result<String, Failure> {
        required_setting(
            explicit
                .map(|value| value.trim().to_string())
                .or_else(|| selected_setting(Some(&self.loaded), Some(SURFACE), name, None)),
            name,
        )
        .map_err(|detail| Failure::config("seo-evaluate", detail))
    }
}

/// The base URL every route hangs off: absolute, credential-free, and
/// either HTTPS or loopback HTTP.
fn canonical_base_url(base_url: &str) -> Result<url::Url, Failure> {
    let canonical = url::Url::parse(base_url).map_err(|_| {
        Failure::invalid(
            "seo-evaluate",
            "invalid SEO contract: base URL must be an absolute URL",
        )
    })?;
    if canonical.username() != "" || canonical.password().is_some() {
        return Err(Failure::invalid(
            "seo-evaluate",
            "invalid SEO contract: base URL must not contain credentials",
        ));
    }
    if canonical.scheme() != "https"
        && !(canonical.scheme() == "http" && canonical.host_str().is_some_and(loopback))
    {
        return Err(Failure::invalid(
            "seo-evaluate",
            "invalid SEO contract: base URL must use HTTPS or loopback HTTP",
        ));
    }
    Ok(canonical)
}

fn read_document(path: &Path, what: &str) -> Result<JsonValue, Failure> {
    serde_json::from_slice(&fs::read(path)?).map_err(|error| {
        Failure::config(
            "seo-evaluate",
            format!("cannot read {what} {}: {error}", path.display()),
        )
    })
}
