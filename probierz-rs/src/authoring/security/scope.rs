//! An operator's exact owned target, not a destination a model may invent.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, net::IpAddr, num::NonZeroUsize};
use reqwest::StatusCode;
use url::Url;

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Scheme { Http, Https }

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub owner: String,
    pub authorization: String,
    pub origin: String,
    pub addresses: Vec<IpAddr>,
    pub response_bytes: NonZeroUsize,
    pub checks: Vec<Check>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Check {
    Headers { id: String, path: String },
    Access { id: String, path: String, expected_status: Vec<u16> },
    Policy { id: String, path: String, expected_status: Vec<u16>, headers: Vec<HeaderRule> },
    Cors { id: String, path: String, denied_origin: String, expected_status: Vec<u16> },
    Cookies { id: String, path: String, expected_status: Vec<u16>, cookies: Vec<CookieRule> },
}
impl Check {
    pub fn id(&self) -> &str {
        match self {
            Self::Headers { id, .. } | Self::Access { id, .. } | Self::Policy { id, .. }
            | Self::Cors { id, .. } | Self::Cookies { id, .. } => id,
        }
    }
    pub fn path(&self) -> &str {
        match self {
            Self::Headers { path, .. } | Self::Access { path, .. } | Self::Policy { path, .. }
            | Self::Cors { path, .. } | Self::Cookies { path, .. } => path,
        }
    }
}

/// Attributes one named Set-Cookie must carry. Every field is declared;
/// `same_site: unchecked` states explicitly that SameSite is not asserted.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CookieRule {
    pub name: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSite,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SameSite { Strict, Lax, None, Unchecked }

impl SameSite {
    /// The attribute value the cookie must carry, or none when unchecked.
    pub fn declared(self) -> Option<&'static str> {
        match self {
            Self::Strict => Some("strict"),
            Self::Lax => Some("lax"),
            Self::None => Some("none"),
            Self::Unchecked => Option::None,
        }
    }
}

impl CookieRule {
    fn validate(&self, check: &str) -> Result<(), String> {
        let separators = b"()<>@,;:\\\"/[]?={}";
        if self.name.is_empty() || self.name.bytes().any(|c| !c.is_ascii_graphic() || separators.contains(&c)) {
            return Err(format!("check {check}: cookie name {:?} must be a nonempty RFC 6265 token", self.name));
        }
        if matches!(self.same_site, SameSite::None) && !self.secure {
            return Err(format!("check {check}: cookie {} declares same_site none, which requires secure: true", self.name));
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "expect", rename_all = "lowercase", deny_unknown_fields)]
pub enum HeaderRule {
    Present { name: String },
    Absent { name: String },
    Equals { name: String, value: String },
}

impl HeaderRule {
    pub fn name(&self) -> &str {
        match self {
            Self::Present { name } | Self::Absent { name } | Self::Equals { name, .. } => name,
        }
    }

    fn validate(&self) -> Result<reqwest::header::HeaderName, String> {
        let name = reqwest::header::HeaderName::from_bytes(self.name().as_bytes())
            .map_err(|error| format!("invalid policy header name {:?}: {error}", self.name()))?;
        if let Self::Equals { value, .. } = self {
            if value.trim().is_empty() {
                return Err(format!("header {name}: expected value must not be empty"));
            }
            reqwest::header::HeaderValue::from_str(value)
                .map_err(|error| format!("header {name}: invalid expected value: {error}"))?;
        }
        Ok(name)
    }
}
impl Scope {
    pub fn validate(&self) -> Result<Url, String> {
        let url = Url::parse(&self.origin).map_err(|e| format!("invalid owned origin: {e}"))?;
        serde_json::from_value::<Scheme>(serde_json::Value::String(url.scheme().into()))
            .map_err(|_| "owned origin scheme must be HTTP or HTTPS".to_string())?;
        if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some()
            || url.path() != "/" || url.query().is_some() || url.fragment().is_some()
            || self.origin.chars().any(char::is_whitespace) || self.origin.contains('\\') {
            return Err("owned origin must not contain credentials, path, query or fragment".into());
        }
        if self.owner.trim().is_empty() || self.authorization.trim().is_empty() {
            return Err("owner and authorization must explicitly attest ownership and permission".into());
        }
        if self.addresses.is_empty() || self.addresses.iter().any(|a| a.is_unspecified() || a.is_multicast()) {
            return Err("addresses must list approved non-multicast, non-unspecified IP addresses".into());
        }
        if self.checks.is_empty() { return Err("checks must list at least one authorized check".into()); }
        let mut ids = BTreeSet::new();
        for check in &self.checks {
            if check.id().is_empty() || !check.id().bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
                return Err("check id must contain only letters, digits, underscores and hyphens".into());
            }
            if !ids.insert(check.id()) { return Err(format!("duplicate check id: {}", check.id())); }
            let path = check.path();
            let joined = url.join(path).map_err(|e| format!("invalid check path: {e}"))?;
            if !path.starts_with('/') || path.starts_with("//") || joined.path() != path
                || path.bytes().any(|c| c.is_ascii_control() || c == b' ' || !c.is_ascii() || b"%?#\\".contains(&c)) {
                return Err(format!("check {}: path must be a canonical absolute path", check.id()));
            }
            if let Check::Access { expected_status, .. } = check {
                let refusals = [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN, StatusCode::NOT_FOUND];
                if expected_status.is_empty() || expected_status.iter().any(|s| !refusals.iter().any(|r| r.as_u16() == *s)) {
                    return Err("access expected_status must list HTTP 401, 403 or 404 refusals".into());
                }
            }
            if let Check::Policy { expected_status, .. } | Check::Cors { expected_status, .. }
                | Check::Cookies { expected_status, .. } = check {
                if expected_status.is_empty() || expected_status.iter().any(|status| StatusCode::from_u16(*status).is_err()) {
                    return Err(format!("check {}: expected_status must list valid HTTP statuses", check.id()));
                }
            }
            if let Check::Policy { headers, .. } = check {
                if headers.is_empty() {
                    return Err(format!("check {}: policy headers must not be empty", check.id()));
                }
                let mut names = BTreeSet::new();
                for rule in headers {
                    let name = rule.validate()?;
                    if !names.insert(name.as_str().to_owned()) {
                        return Err(format!("check {}: duplicate policy header {name}", check.id()));
                    }
                }
            }
            if let Check::Cors { denied_origin, .. } = check {
                let foreign = Url::parse(denied_origin)
                    .map_err(|error| format!("check {}: invalid denied_origin: {error}", check.id()))?;
                serde_json::from_value::<Scheme>(serde_json::Value::String(foreign.scheme().into()))
                    .map_err(|_| format!("check {}: denied_origin must use HTTP or HTTPS", check.id()))?;
                if foreign.origin().ascii_serialization() != *denied_origin || foreign.origin() == url.origin() {
                    return Err(format!("check {}: denied_origin must be a canonical origin distinct from the owned origin", check.id()));
                }
            }
            if let Check::Cookies { cookies, .. } = check {
                if cookies.is_empty() {
                    return Err(format!("check {}: cookies must not be empty", check.id()));
                }
                let mut names = BTreeSet::new();
                for rule in cookies {
                    rule.validate(check.id())?;
                    if !names.insert(rule.name.as_str()) {
                        return Err(format!("check {}: duplicate cookie {}", check.id(), rule.name));
                    }
                }
            }
        }
        Ok(url)
    }
    pub fn selected(&self, selection: &[String]) -> Result<Vec<&Check>, String> {
        let expected: BTreeSet<_> = self.checks.iter().map(Check::id).collect();
        let actual: BTreeSet<_> = selection.iter().map(String::as_str).collect();
        if selection.len() != expected.len() || actual != expected {
            return Err("model selection must contain every declared check exactly once, and no others".into());
        }
        selection.iter().map(|id| self.checks.iter().find(|check| check.id() == id)
            .ok_or_else(|| format!("model selected undeclared check {id}"))).collect()
    }
}
