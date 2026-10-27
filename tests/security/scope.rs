//! Consumer-visible authorization boundaries, independent of model availability.
#[path = "../../probierz-rs/src/authoring/security/scope.rs"]
#[allow(dead_code)]
mod scope;
use scope::{Check, HeaderRule, Scope};
#[path = "../../probierz-rs/src/authoring/security/probe.rs"]
#[allow(dead_code)]
mod probe;

fn owned() -> Scope {
    Scope {
        owner: "our test application".into(), authorization: "owned isolated test target".into(),
        origin: "http://localhost".into(), addresses: vec![std::net::Ipv4Addr::LOCALHOST.into()],
        response_bytes: std::num::NonZeroUsize::MIN,
        checks: vec![Check::Headers { id: "headers".into(), path: "/".into() },
            Check::Access { id: "private".into(), path: "/private".into(),
                expected_status: vec![reqwest::StatusCode::UNAUTHORIZED.as_u16()] }],
    }
}
#[test]
fn paths_cannot_escape_the_authorized_origin_or_decode_to_other_routes() {
    for path in ["//elsewhere.invalid/", "https://elsewhere.invalid/", "/a/../private", "/%2e%2e/private", "/a?redirect=elsewhere", "/a#fragment", "/a\\b", "/a\r\nHost:elsewhere.invalid"] {
        let mut target = owned();
        target.checks = vec![Check::Headers { id: "escape".into(), path: path.into() }];
        assert!(target.validate().is_err(), "accepted escaping path {path:?}");
    }
    assert!(owned().validate().is_ok());
}
#[test]
fn declaration_requires_ownership_and_exact_addresses() {
    let mut target = owned();
    target.authorization.clear();
    assert!(target.validate().is_err());
    target = owned(); target.owner.clear();
    assert!(target.validate().is_err());
    target = owned(); target.addresses.clear();
    assert!(target.validate().is_err());
    target = owned(); target.addresses = vec![std::net::Ipv4Addr::UNSPECIFIED.into()];
    assert!(target.validate().is_err());
}
#[test]
fn model_must_select_every_declared_check_once_without_inventing_checks() {
    let target = owned();
    let declared: Vec<String> = target.checks.iter().map(|check| check.id().to_owned()).collect();
    let mut omitted = declared.clone(); omitted.pop();
    let duplicated = target.checks.iter().map(|_| declared.first().expect("declared check").clone()).collect::<Vec<_>>();
    let mut invented = declared.clone(); invented.push(target.owner.clone());
    for selection in [omitted, duplicated, invented] {
        assert!(target.selected(&selection).is_err());
    }
    let reversed: Vec<String> = declared.into_iter().rev().collect();
    let selected = target.selected(&reversed).expect("valid reordered plan");
    assert_eq!(selected.iter().map(|check| check.id()).collect::<Vec<_>>(), reversed.iter().map(String::as_str).collect::<Vec<_>>());
}
#[test]
fn access_check_cannot_call_success_an_authorization_refusal() {
    let mut target = owned();
    target.checks = vec![Check::Access { id: "private".into(), path: "/private".into(),
        expected_status: vec![reqwest::StatusCode::OK.as_u16()] }];
    assert!(target.validate().is_err());
}

#[test]
fn a_resolved_address_outside_the_allowlist_is_refused_before_http() {
    let mut target = owned();
    target.origin = format!("http://{}", std::net::Ipv4Addr::LOCALHOST);
    target.addresses = vec![std::net::Ipv6Addr::LOCALHOST.into()];
    let failure = probe::Probe::new(&target).err().expect("IPv4 target cannot use IPv6-only authorization");
    assert!(failure.contains("outside the approved address set"), "{failure}");
}

#[test]
fn policy_rejects_ambiguous_headers_and_injected_values() {
    let mut target = owned();
    for headers in [
        vec![HeaderRule::Present { name: "Content-Security-Policy".into() },
            HeaderRule::Absent { name: "content-security-policy".into() }],
        vec![HeaderRule::Equals { name: "x-content-type-options".into(), value: "nosniff\r\ninjected: yes".into() }],
        vec![HeaderRule::Absent { name: "bad header".into() }],
        Vec::new(),
    ] {
        target.checks = vec![Check::Policy {
            id: "policy".into(), path: "/".into(),
            expected_status: vec![reqwest::StatusCode::OK.as_u16()], headers,
        }];
        assert!(target.validate().is_err());
    }
}

#[test]
fn cors_origin_is_an_explicit_foreign_origin_not_a_target_url() {
    let mut target = owned();
    for denied_origin in ["http://localhost", "https://foreign.invalid/path",
        "file:///private", "https://foreign.invalid/"] {
        target.checks = vec![Check::Cors {
            id: "cors".into(), path: "/".into(), denied_origin: denied_origin.into(),
            expected_status: vec![reqwest::StatusCode::OK.as_u16()],
        }];
        assert!(target.validate().is_err(), "accepted {denied_origin}");
    }
    target.checks = vec![Check::Cors {
        id: "cors".into(), path: "/".into(), denied_origin: "https://foreign.invalid".into(),
        expected_status: Vec::new(),
    }];
    assert!(target.validate().is_err(), "CORS must not guess acceptable statuses");
}
