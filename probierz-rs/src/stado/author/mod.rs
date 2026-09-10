//! Remote authoring: the inputs a job is given, the receipt it writes, and
//! the remote half that submits it and brings the result back.

mod inputs;
mod receipt;
mod remote;

pub(crate) use inputs::*;
pub(crate) use receipt::*;
pub(crate) use remote::*;
