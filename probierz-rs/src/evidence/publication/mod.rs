//! Publishing what a verified run proves: the command an operator reaches
//! it through and the document that command creates.

mod document;
mod entry;

pub(crate) use document::*;
pub use entry::*;
