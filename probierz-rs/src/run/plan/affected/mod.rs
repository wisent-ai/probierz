//! Which targets and journeys a set of changed files selects: the path
//! rules that decide whether a file belongs to a declaration, and the
//! selection those rules produce.

mod paths;
mod selection;

pub(crate) use paths::*;
pub(crate) use selection::*;
