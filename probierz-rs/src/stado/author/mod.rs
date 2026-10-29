//! Remote authoring: the inputs a job is given, the receipt it writes, and
//! the remote half that submits it and brings the result back.

mod inputs;
mod receipt;
mod remote;

pub(crate) use inputs::*;
pub(crate) use receipt::*;
pub(crate) use remote::*;

/// The schema of an accepted authoring receipt, the one `write_author_receipt`
/// writes and the remote restore matches.
const AUTHOR_RECEIPT_SCHEMA_VERSION: u64 = 1;
