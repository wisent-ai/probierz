//! The answers a journey gives in place of a live feed while it proves the
//! product waits for real data. These constants are the held-back responses:
//! a failing report, an empty offer list, an empty token list.

pub(super) const UNAVAILABLE: u16 = 503;
pub(super) const OK: u16 = 200;
pub(super) const JSON: &str = "application/json";

pub(super) const REPORT_HELD_BACK: &str = r#"{"error":"resume-boundary"}"#;
pub(super) const NO_OFFERS: &str = r#"{"offers":[],"total":0}"#;
pub(super) const NO_TOKENS: &str = "[]";
