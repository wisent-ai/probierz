//! The declared SEO contract: what it requires, the model that reads a
//! crawled page against it, and the signed verdict.

mod contract;
mod evaluate;
mod model;

pub use evaluate::*;
pub(crate) use contract::*;
pub(crate) use model::*;
