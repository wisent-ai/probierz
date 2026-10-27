use crate::manifest::*;

/// The modes an SEO evaluation runs in, which are also the profiles a
/// manifest's `seo.profiles` may declare: one per kind of release decision.
/// `production` additionally requires Search Console and CrUX evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SeoMode {
    PullRequest,
    Release,
    Nightly,
    Production,
}

impl SeoMode {
    /// The mode `name` names, or serde's refusal, which lists every mode.
    pub fn parse(name: &Value) -> Result<Self, String> {
        serde_yaml::from_value(name.clone()).map_err(|error| error.to_string())
    }
}
