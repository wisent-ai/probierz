//! Exact-origin GET checks. No proxy, redirects, cookie jar or model commands.
use super::scope::{Check, CookieRule, HeaderRule, Scope};
use serde_json::{json, Value};
use std::{io::Read, net::ToSocketAddrs};

pub struct Probe { client: reqwest::blocking::Client, origin: url::Url }
impl Probe {
    pub fn new(scope: &Scope) -> Result<Self, String> {
        let origin = scope.validate()?;
        let host = origin.host_str().ok_or("owned origin has no host")?;
        let port = origin.port_or_known_default().ok_or("owned origin has no port")?;
        let addresses = stado_wait::blocking(stado_wait::Kind::Network, "resolve owned target", host,
            || (host.trim_matches(['[', ']']), port).to_socket_addrs())
            .map_err(|e| format!("owned target DNS resolution failed: {e}"))?.collect::<Vec<_>>();
        if addresses.is_empty() || addresses.iter().any(|a| !scope.addresses.contains(&a.ip())) {
            return Err(format!("owned target DNS addresses {addresses:?} are outside the approved address set"));
        }
        let client = reqwest::blocking::Client::builder().no_proxy().timeout(None)
            .redirect(reqwest::redirect::Policy::none()).resolve_to_addrs(host, &addresses)
            .build().map_err(|e| format!("creating scoped HTTP client failed: {e}"))?;
        Ok(Self { client, origin })
    }
    pub fn check(&self, scope: &Scope, check: &Check) -> Result<Value, String> {
        let url = self.origin.join(check.path()).map_err(|e| e.to_string())?;
        // The check's causes are text, which stado_wait::blocking cannot carry
        // (it settles a std::error::Error), so the wait is begun and settled here.
        let waiting = stado_wait::begin(stado_wait::Kind::Network, format!("owned check {}", check.id()), url.as_str());
        let answer = (|| -> Result<Value, String> {
            let mut request = self.client.get(url.clone());
            if let Check::Cors { denied_origin, .. } = check {
                request = request.header(reqwest::header::ORIGIN, denied_origin);
            }
            let mut response = request.send().map_err(|e| format!("GET {url} failed: {e}"))?;
            let status = response.status();
            let mut missing = Vec::new();
            if matches!(check, Check::Headers { .. }) {
                for name in ["content-security-policy", "x-content-type-options"] {
                    if !response.headers().contains_key(name) { missing.push(name); }
                }
                if self.origin.scheme() == "https" && !response.headers().contains_key("strict-transport-security") {
                    missing.push("strict-transport-security");
                }
            }
            let header_bytes: usize = response.headers().iter().map(|(name, value)| name.as_str().len() + value.len()).sum();
            if header_bytes > scope.response_bytes.get() { return Err("response headers exceed the declared response_bytes limit".into()); }
            let mut failed_assertions = Vec::new();
            if let Check::Policy { headers, .. } = check {
                for rule in headers {
                    let mut values = response.headers().get_all(rule.name()).iter();
                    let failure = match rule {
                        HeaderRule::Present { .. } => values.next().is_none()
                            .then_some(("missing", "the required header is absent")),
                        HeaderRule::Absent { .. } => values.next().is_some()
                            .then_some(("unexpected", "the forbidden header is present")),
                        HeaderRule::Equals { value, .. } => match values.next() {
                            None => Some(("missing", "the exact-value assertion received no header")),
                            Some(_) if values.next().is_some() =>
                                Some(("repeated", "the exact-value assertion received multiple header values")),
                            Some(actual) if actual.as_bytes() != value.as_bytes() =>
                                Some(("value-mismatch", "the header differs from the declared exact value")),
                            Some(_) => None,
                        },
                    };
                    if let Some((reason, cause)) = failure {
                        failed_assertions.push(json!({"header": rule.name(), "reason": reason, "cause": cause}));
                    }
                }
            }
            if let Check::Cors { denied_origin, .. } = check {
                if response.headers().get_all(reqwest::header::ACCESS_CONTROL_ALLOW_ORIGIN).iter()
                    .any(|value| value.as_bytes() == b"*" || value.as_bytes() == denied_origin.as_bytes()) {
                    failed_assertions.push(json!({"header": "access-control-allow-origin", "reason": "denied-origin-allowed",
                        "cause": "the response allows the declared denied origin or a wildcard"}));
                }
            }
            let failed_cookies: Vec<Value> = match check {
                Check::Cookies { cookies, .. } =>
                    cookies.iter().flat_map(|rule| cookie_failures(response.headers(), rule)).collect(),
                _ => Vec::new(),
            };
            // Discard the body; neither secrets nor prompt injection reach the model.
            let limit = u64::try_from(scope.response_bytes.get()).map_err(|e| e.to_string())?;
            std::io::copy(&mut response.by_ref().take(limit), &mut std::io::sink())
                .map_err(|e| format!("reading GET {url} response failed: {e}"))?;
            if Read::bytes(response).next().transpose().map_err(|e| e.to_string())?.is_some() {
                return Err("response body exceeds the declared response_bytes limit".into());
            }
            let passed = match check {
                Check::Headers { .. } => status.is_success() && missing.is_empty(),
                Check::Access { expected_status, .. } => expected_status.contains(&status.as_u16()),
                Check::Policy { expected_status, .. } => {
                    expected_status.contains(&status.as_u16()) && failed_assertions.is_empty()
                }
                Check::Cors { expected_status, .. } => expected_status.contains(&status.as_u16())
                    && failed_assertions.is_empty(),
                Check::Cookies { expected_status, .. } => expected_status.contains(&status.as_u16())
                    && failed_cookies.is_empty(),
            };
            Ok(json!({"id": check.id(), "method": "GET", "path": check.path(), "status": status.as_u16(),
                "pass": passed, "missingHeaders": missing, "failedAssertions": failed_assertions,
                "failedCookies": failed_cookies, "redirectNotFollowed": status.is_redirection()}))
        })();
        match &answer {
            Ok(_) => waiting.done(),
            Err(cause) => waiting.failed(cause),
        }
        answer
    }
}

/// How the one Set-Cookie carrying `rule`'s name breaks its declared
/// attributes. Cookie values are never read into the result.
fn cookie_failures(headers: &reqwest::header::HeaderMap, rule: &CookieRule) -> Vec<Value> {
    let set: Vec<Vec<(String, Option<String>)>> = headers.get_all(reqwest::header::SET_COOKIE).iter()
        .filter_map(|value| {
            let mut parts = value.as_bytes().split(|byte| *byte == b';');
            let pair = parts.next()?;
            let name = pair.split(|byte| *byte == b'=').next()?.trim_ascii();
            (name == rule.name.as_bytes()).then(|| parts.map(attribute).collect())
        })
        .collect();
    let finding = |reason: &str, cause: &str| json!({"cookie": rule.name, "reason": reason, "cause": cause});
    let attributes = match set.as_slice() {
        [] => return vec![finding("missing", "the response sets no cookie with the declared name")],
        [one] => one,
        _ => return vec![finding("repeated", "the response sets the declared cookie more than once")],
    };
    let has = |key: &str| attributes.iter().any(|(name, _)| name == key);
    let mut failures = Vec::new();
    if rule.secure && !has("secure") {
        failures.push(finding("not-secure", "the cookie lacks the Secure attribute"));
    }
    if rule.http_only && !has("httponly") {
        failures.push(finding("not-httponly", "the cookie lacks the HttpOnly attribute"));
    }
    if let Some(wanted) = rule.same_site.declared() {
        let declared: Vec<_> = attributes.iter().filter(|(name, _)| name == "samesite").collect();
        if !matches!(declared.as_slice(), [(_, Some(value))] if value.eq_ignore_ascii_case(wanted)) {
            failures.push(finding("samesite-mismatch",
                "the cookie's SameSite attribute is absent, repeated or differs from the declared value"));
        }
    }
    failures
}

/// One cookie attribute: its lowercase name and, when it has one, its value
/// (everything after the first `=`).
fn attribute(part: &[u8]) -> (String, Option<String>) {
    let text = String::from_utf8_lossy(part);
    let (name, value) = match text.split_once('=') {
        Some((name, value)) => (name, Some(value.trim().to_string())),
        None => (text.as_ref(), None),
    };
    (name.trim().to_ascii_lowercase(), value)
}
