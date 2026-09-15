//! The declared SEO contract: what it requires, the model that reads a
//! crawled page against it, the evaluation, and the signed verdict.

mod contract;
mod evaluate;
mod model;
mod report;

// Only the command is exported; the evaluation's own stages stay
// inside the package, so a name like `Contract` or `Graded` cannot
// collide with another evaluation's.
pub use evaluate::evaluate_seo;
pub(crate) use contract::*;
pub(crate) use model::*;
pub(crate) use report::*;
