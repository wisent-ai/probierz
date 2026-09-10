//! The JSON-RPC side: the wire it is written on, the command each tool call
//! becomes, and the dispatch loop that serves them.

mod dispatch;
mod routes;
mod wire;

pub(crate) use dispatch::*;
pub(crate) use routes::*;
pub(crate) use wire::*;
