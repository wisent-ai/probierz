//! The Figma parity journey's constants: how long preparing the package may
//! take, and the port its test server listens on.

/// `npm ci` and `npm run build` of the package get ten minutes each.
pub(super) const PREPARE_SECONDS: u64 = 600;
/// The package's test server (tests/visual/server.mjs) reads PORT.
pub(super) const SERVER_PORT: &str = "4173";
