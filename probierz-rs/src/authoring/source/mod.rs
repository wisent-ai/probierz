//! What a product's source is: the files that count, the identity they hash
//! to, and the accessibility identifiers declared inside them.

mod accessibility;
mod accessibility_scan;
mod files;
mod identity;

pub use accessibility::*;
pub use identity::*;
pub(crate) use accessibility_scan::*;
pub(crate) use files::*;
