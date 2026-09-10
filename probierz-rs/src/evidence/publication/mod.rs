//! Publishing what a verified run proves: the command an operator reaches
//! it through, the document that command creates, and the onboarding
//! publication beside it.

mod document;
mod entry;
mod onboarding;

pub use entry::*;
pub use onboarding::*;
pub(crate) use document::*;
